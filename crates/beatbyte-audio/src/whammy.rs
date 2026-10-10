//! A two-head variable-delay pitch bend. Consumes exactly one source sample
//! per output sample: neither song duration nor its clock changes.
use rodio::Source;
use std::sync::{
    Arc,
    atomic::{AtomicU32, Ordering},
};
use std::time::Duration;

/// Streaming pitch bend with smoothed pressure and channel-preserving delay heads.
pub struct WhammySource<I> {
    inner: I,
    control: Arc<AtomicU32>,
    ring: Vec<f32>,
    frames: usize,
    channels: usize,
    frame: usize,
    channel: usize,
    phase: f32,
    depth: f32,
    smooth: f32,
    window: f32,
}

impl<I: Source> WhammySource<I> {
    /// Wrap a source; control contains the bit representation of pressure in 0..1.
    pub fn new(inner: I, control: Arc<AtomicU32>) -> Self {
        let rate = inner.sample_rate().get() as f32;
        let channels = inner.channels().get() as usize;
        let window = (rate * 0.04).max(8.0);
        let frames = window.ceil() as usize + 4;
        Self {
            inner,
            control,
            ring: vec![0.0; frames * channels],
            frames,
            channels,
            frame: 0,
            channel: 0,
            phase: 0.0,
            depth: 0.0,
            smooth: 1.0 - (-1.0 / (rate * 0.01)).exp(),
            window,
        }
    }
    fn delayed(&self, phase: f32) -> f32 {
        let position =
            (self.frame as f32 - 1.0 - phase * self.window).rem_euclid(self.frames as f32);
        let a = position.floor() as usize;
        let b = (a + 1) % self.frames;
        let mix = position - a as f32;
        self.ring[a * self.channels + self.channel] * (1.0 - mix)
            + self.ring[b * self.channels + self.channel] * mix
    }
}
impl<I: Source> Iterator for WhammySource<I> {
    type Item = f32;
    fn next(&mut self) -> Option<f32> {
        let dry = self.inner.next()?;
        if self.channel == 0 {
            let requested = f32::from_bits(self.control.load(Ordering::Relaxed));
            let target = if requested.is_finite() {
                requested.clamp(0.0, 1.0)
            } else {
                0.0
            };
            self.depth += (target - self.depth) * self.smooth;
            if target == 0.0 && self.depth < 0.00001 {
                self.depth = 0.0;
            }
        }
        self.ring[self.frame * self.channels + self.channel] = dry;
        let out = if self.depth == 0.0 {
            dry
        } else {
            let weight = 1.0 - (2.0 * self.phase - 1.0).abs();
            let wet = self.delayed(self.phase) * weight
                + self.delayed((self.phase + 0.5).fract()) * (1.0 - weight);
            // Crossfade into the bend without clicks as the bar is engaged.
            let mix = (self.depth * 10.0).min(1.0);
            dry * (1.0 - mix) + wet * mix
        };
        self.channel += 1;
        if self.channel == self.channels {
            self.channel = 0;
            self.frame = (self.frame + 1) % self.frames;
            let ratio = 2.0_f32.powf(-2.0 * self.depth / 12.0);
            self.phase = (self.phase + (1.0 - ratio) / self.window).fract();
        }
        Some(out)
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        self.inner.size_hint()
    }
}
impl<I: Source> Source for WhammySource<I> {
    fn current_span_len(&self) -> Option<usize> {
        self.inner.current_span_len()
    }
    fn channels(&self) -> rodio::ChannelCount {
        self.inner.channels()
    }
    fn sample_rate(&self) -> rodio::SampleRate {
        self.inner.sample_rate()
    }
    fn total_duration(&self) -> Option<Duration> {
        self.inner.total_duration()
    }
    fn try_seek(&mut self, pos: Duration) -> Result<(), rodio::source::SeekError> {
        self.inner.try_seek(pos)?;
        self.ring.fill(0.0);
        self.frame = 0;
        self.channel = 0;
        self.phase = 0.0;
        self.depth = 0.0;
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn source(samples: Vec<f32>, channels: u16) -> rodio::buffer::SamplesBuffer {
        rodio::buffer::SamplesBuffer::new(
            channels.try_into().expect("nonzero channels"),
            48000.try_into().expect("nonzero sample rate"),
            samples,
        )
    }
    #[test]
    fn idle_is_bit_exact_and_preserves_duration_and_sample_count() {
        let data = vec![0.3, -0.2, 0.4, 0.7];
        let src = source(data.clone(), 2);
        let duration = src.total_duration();
        let bend = WhammySource::new(src, Arc::new(AtomicU32::new(0.0_f32.to_bits())));
        assert_eq!(bend.total_duration(), duration);
        assert_eq!(bend.collect::<Vec<_>>(), data);
    }
    #[test]
    fn full_travel_lowers_pitch_without_changing_length_or_stereo_routing() {
        let data: Vec<f32> = (0..96000)
            .flat_map(|i| {
                let x = (std::f32::consts::TAU * 440.0 * i as f32 / 48000.0).sin();
                [x, -x]
            })
            .collect();
        let out: Vec<_> = WhammySource::new(
            source(data.clone(), 2),
            Arc::new(AtomicU32::new(1.0_f32.to_bits())),
        )
        .collect();
        assert_eq!(out.len(), data.len());
        for pair in out.as_chunks::<2>().0 {
            assert!((pair[0] + pair[1]).abs() < 0.00001);
        }
        let mono: Vec<_> = out
            .as_chunks::<2>()
            .0
            .iter()
            .skip(48000)
            .map(|p| p[0])
            .collect();
        let crossings = mono
            .windows(2)
            .filter(|p| p[0] <= 0.0 && p[1] > 0.0)
            .count();
        assert!(
            (385..405).contains(&crossings),
            "pitch {crossings} Hz, expected ~392"
        );
        assert!(out.iter().all(|x| x.is_finite() && x.abs() <= 1.0));
    }
    #[test]
    fn release_returns_to_exact_bypass() {
        let control = Arc::new(AtomicU32::new(1.0_f32.to_bits()));
        let mut bend = WhammySource::new(source(vec![0.25; 48000], 1), control.clone());
        for _ in 0..12000 {
            bend.next();
        }
        control.store(0.0_f32.to_bits(), Ordering::Relaxed);
        for _ in 0..12000 {
            bend.next();
        }
        assert_eq!(bend.depth, 0.0);
        assert!(bend.all(|x| x == 0.25));
    }
}
