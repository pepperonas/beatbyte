//! Asking a music catalogue what a song is.
//!
//! **Optional, and encapsulated here.** A song is fully playable
//! with no network at all; nothing in the game or the offline tool
//! calls this unless somebody asks it to. The crate is not in the
//! default build (`--features catalogue`), it talks to exactly one
//! service, and what it sends is an artist and a title.
//!
//! ⚠️ **A catalogue answer is a claim, not a fact.** Asked for David
//! Bowie's "Heroes", MusicBrainz returns eight recordings, **every
//! one of them scored 100**, running from 0 to 393 seconds — live
//! takes, a 2021 re-release, the album version, the single edit. The
//! score cannot tell them apart. The song's own length can, and
//! that is what decides here; the score is only a tie-break.
//!
//! The same trap sits one level down. The single edit's first listed
//! release is a DJ compilation with no date, so a reader that took
//! `releases[0]` would record *Mastermix Classic Cuts, Volume 129:
//! Stag Night* as the album — worse than recording nothing.
//!
//! Everything that decides is pure and tested against a real
//! recorded response. Only [`Client`] touches the network.

mod client;
mod pick;

pub use client::{Client, MIN_INTERVAL, user_agent};
pub use pick::{
    FALLBACK_CONFIDENCE, Match, Recording, Release, parse, pick, pick_release, release_year,
};
