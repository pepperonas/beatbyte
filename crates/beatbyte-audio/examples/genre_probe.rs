//! Print the genre tag of the given audio files — a field check for
//! `read_genre` against real files.
fn main() {
    for arg in std::env::args().skip(1) {
        println!(
            "{arg}: {:?}",
            beatbyte_audio::read_genre(std::path::Path::new(&arg))
        );
    }
}
