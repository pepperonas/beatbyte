//! Opt-in, single-source charting experiment. The normal import generator
//! deliberately does not call this until a human has compared the pilot.

use super::*;
use beatbyte_core::music::MelodyNote;

/// Generate an experimental chart from a tonal instrument reading.
///
/// Supply the song's existing beat grid and a melody/onset analysis of ONE
/// instrument source. A full mix is accepted for comparison, but this function
/// does not identify guitars or separate instruments. It uses only supported
/// pitches, leaves tonal rests empty, preserves repeated-pitch strums and
/// suppresses strength-invented chords. The default import path is unchanged.
#[must_use]
pub fn generate_lead_study(analysis: &SongAnalysis, meta: &GenerateMeta) -> ChartFile {
    let mut lead = analysis.clone();
    lead.melody = articulated_notes(analysis);
    lead.onsets.clear();
    // Whole-mix repeat similarity is not evidence that a guitar fill repeats.
    lead.repeats.clear();
    let origin = lead.beats.first().copied().unwrap_or(0.0);
    let selected = select_candidates(&lead, origin);
    let grid = BeatGrid::from_beats(&lead.beats);
    let mut master = Vec::with_capacity(selected.len());
    let mut start = 0;
    while start < selected.len() {
        let mut end = start + 1;
        while end < selected.len() {
            let beat = grid
                .beat_length_at(selected[end - 1].time_s)
                .unwrap_or(lead.beat_interval_s());
            if selected[end].time_s - selected[end - 1].time_s > 4.0 * beat {
                break;
            }
            end += 1;
        }
        // Look ahead over a phrase: one pitch keeps one fret throughout it.
        // A running interval accumulator drifted whenever a leap hit an edge.
        let mut pitches: Vec<i32> = selected[start..end]
            .iter()
            .filter_map(|n| n.pitch.map(|p| p.midi.round() as i32))
            .collect();
        pitches.sort_unstable();
        pitches.dedup();
        for note in &selected[start..end] {
            let Some(pitch) = note.pitch else { continue };
            let rank = pitches.partition_point(|p| *p < pitch.midi.round() as i32);
            let lane = if pitches.len() <= 1 {
                2
            } else {
                (rank as f64 * 4.0 / (pitches.len() - 1) as f64).round() as i32
            };
            let beat = grid
                .beat_length_at(note.time_s)
                .unwrap_or(lead.beat_interval_s());
            master.push(MasterNote {
                time_s: note.time_s,
                strength: note.strength,
                lane,
                held_s: natural_tail(&lead, note, beat),
                pitched: true,
            });
        }
        start = end;
    }
    // A fixed pitch mapping must not demand a whole-hand leap in a sixteenth.
    // Thin the master instead of falsifying a pitch; all levels inherit it.
    limit_fast_reaches(&mut master);
    // Reuse the existing format/metadata and difficulty reduction machinery.
    let mut chart = generate_chart(
        &SongAnalysis {
            melody: Vec::new(),
            ..lead.clone()
        },
        meta,
    );
    chart.charts = Difficulty::ALL
        .iter()
        .map(|&difficulty| {
            let mut profile = DifficultyProfile::for_difficulty(difficulty);
            profile.chord_share = 0.0;
            profile.max_chord_size = 1;
            let notes = derive_notes(&lead, &profile, &master, origin);
            let phrases = place_phrases(&lead, &notes);
            ChartDef {
                difficulty,
                lanes: LANE_COUNT as u8,
                notes,
                phrases,
            }
        })
        .collect();
    chart
}

/// Split a held pitch only on a strong attack from that same source. Onsets
/// outside voiced spans never become notes; a drum-only passage remains empty.
fn articulated_notes(analysis: &SongAnalysis) -> Vec<MelodyNote> {
    let mut out = Vec::new();
    for note in &analysis.melody {
        if !note.time_s.is_finite()
            || !note.end_s.is_finite()
            || !note.midi.is_finite()
            || !note.strength.is_finite()
            || note.time_s < 0.0
            || note.end_s <= note.time_s
            || note.end_s > analysis.duration_s
        {
            continue;
        }
        let mut current = *note;
        for attack in &analysis.onsets {
            if attack.strength < 0.20
                || !attack.strength.is_finite()
                || attack.time_s - current.time_s < 2.0 * ATTACH_S
                || note.end_s - attack.time_s < MASTER_MIN_SPACING_S
                || !attack.time_s.is_finite()
            {
                continue;
            }
            current.end_s = attack.time_s;
            out.push(current);
            current = MelodyNote {
                time_s: attack.time_s,
                end_s: note.end_s,
                midi: note.midi,
                strength: note.strength.min(attack.strength),
            };
        }
        out.push(current);
    }
    out.sort_by(|a, b| a.time_s.total_cmp(&b.time_s));
    out
}

fn limit_fast_reaches(master: &mut Vec<MasterNote>) {
    let mut previous: Option<(f64, i32)> = None;
    master.retain(|note| {
        if previous.is_some_and(|(time, lane)| {
            note.time_s - time < JACK_GAP_S && (note.lane - lane).abs() > 2
        }) {
            return false;
        }
        previous = Some((note.time_s, note.lane));
        true
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reading(pitches: &[f32]) -> SongAnalysis {
        let mut a = super::super::tests::analysis();
        a.onsets.clear();
        a.melody = pitches
            .iter()
            .enumerate()
            .map(|(i, &midi)| MelodyNote {
                time_s: 1.0 + i as f64 * 0.125,
                end_s: 1.1 + i as f64 * 0.125,
                midi,
                strength: 0.9,
            })
            .collect();
        a
    }

    fn chart(a: &SongAnalysis) -> ChartFile {
        generate_lead_study(
            a,
            &GenerateMeta {
                title: "Study".into(),
                artist: "Test".into(),
                audio: "mix.wav".into(),
            },
        )
    }

    #[test]
    fn unpitched_hits_do_not_fill_instrument_rests() {
        let a = super::super::tests::analysis();
        assert!(chart(&a).charts.iter().all(|c| c.notes.is_empty()));
    }

    #[test]
    fn repeated_pitches_stay_strums_and_return_to_the_same_fret() {
        let mut a = reading(&[60.0, 60.0, 67.0, 60.0, 64.0, 60.0]);
        for (i, note) in a.melody.iter_mut().enumerate().skip(2) {
            note.time_s += i as f64 * 0.125;
            note.end_s += i as f64 * 0.125;
        }
        let c = chart(&a);
        let n = &c.chart_for(Difficulty::Expert).expect("expert").notes;
        assert_eq!(n.len(), 6);
        assert_eq!(n[0].lane, n[1].lane);
        assert!(!n[1].hopo);
        for i in [3, 5] {
            assert_eq!(n[0].lane, n[i].lane);
        }
        assert!(n[2].lane > n[4].lane && n[4].lane > n[0].lane);
    }

    #[test]
    fn a_new_source_attack_ends_the_previous_hold() {
        let mut a = reading(&[60.0]);
        a.melody[0].end_s = 3.0;
        a.onsets = vec![beatbyte_core::music::Onset {
            time_s: 2.0,
            strength: 0.8,
            brightness: 0.5,
        }];
        let notes = articulated_notes(&a);
        assert_eq!(notes.len(), 2);
        assert_eq!(notes[0].end_s, notes[1].time_s);
        assert_eq!(notes[0].midi, notes[1].midi);
    }

    #[test]
    fn a_fast_whole_hand_leap_is_thinned_without_moving_the_pitch() {
        let mut master: Vec<_> = [(1.0, 0), (1.11, 4), (1.25, 4), (1.36, 4)]
            .into_iter()
            .map(|(time_s, lane)| MasterNote {
                time_s,
                lane,
                strength: 0.9,
                held_s: 0.0,
                pitched: true,
            })
            .collect();
        limit_fast_reaches(&mut master);
        assert_eq!(
            master
                .iter()
                .map(|n| (n.time_s, n.lane))
                .collect::<Vec<_>>(),
            vec![(1.0, 0), (1.25, 4), (1.36, 4)]
        );
    }

    #[test]
    fn reductions_are_nested_valid_and_never_invent_chords() {
        let mut a = reading(&[60.0, 62.0, 64.0, 65.0, 67.0, 65.0, 64.0, 62.0]);
        for (i, n) in a.melody.iter_mut().enumerate() {
            n.strength = 0.4 + i as f32 * 0.07;
        }
        let c = chart(&a);
        assert_eq!(c, chart(&a));
        assert!(
            !c.validate()
                .iter()
                .any(|i| i.severity == crate::Severity::Error)
        );
        for def in &c.charts {
            assert!(def.notes.windows(2).all(|p| p[1].time > p[0].time));
        }
        for pair in c.charts.windows(2) {
            for note in &pair[0].notes {
                assert!(pair[1].notes.iter().any(|n| n.time == note.time));
            }
        }
    }
}
