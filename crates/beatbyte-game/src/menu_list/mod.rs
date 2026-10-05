//! Menus as data: a list screen is a table of rows, and what a row
//! does — show its value, step it, make its sound — is decided here
//! once instead of in a match arm per row on every screen.
//!
//! `spec` is the model, free of Bevy and tested on its own. The
//! settings screen's forty rows are the first table (`settings_ui`),
//! and the pause menu reads the same rows.

pub mod list;
pub mod spec;
