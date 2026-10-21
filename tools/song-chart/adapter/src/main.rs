use beatbyte_audio::{Analyzer, SpectralAnalyzer, decode_file};
use beatbyte_chart::{GenerateMeta, Severity};
use beatbyte_ml::{ModelStore, Runtime};
use serde_json::{Value, json};
use std::{env, fs, path::Path, sync::atomic::AtomicBool};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
fn required<'a>(args: &'a [String], i: usize) -> Result<&'a str> {
    args.get(i)
        .map(String::as_str)
        .ok_or_else(|| format!("missing argument {i}").into())
}
fn main() {
    if let Err(e) = run() {
        eprintln!("adapter: {e}");
        std::process::exit(1);
    }
}
fn run() -> Result<()> {
    let a: Vec<_> = env::args().collect();
    let command = required(&a, 1)?;
    if command == "validate" {
        let chart = beatbyte_chart::load_chart_file(Path::new(required(&a, 2)?))?;
        let issues = chart.validate();
        println!(
            "{}",
            json!({"errors":issues.iter().filter(|i|i.severity == Severity::Error).map(|i|i.to_string()).collect::<Vec<_>>(), "warnings":issues.iter().filter(|i|i.severity != Severity::Error).map(|i|i.to_string()).collect::<Vec<_>>() })
        );
        if issues.iter().any(|i| i.severity == Severity::Error) {
            return Err("chart validation failed".into());
        }
        return Ok(());
    }
    if command == "source-info" {
        let path = Path::new(required(&a, 2)?);
        let song = if path.extension().and_then(|s| s.to_str()) == Some("mid") {
            beatbyte_chart::bridge::midi::parse_midi(&fs::read(path)?)?
        } else {
            beatbyte_chart::bridge::text::parse_chart(&fs::read_to_string(path)?)?
        };
        println!(
            "{}",
            json!({"original_difficulties": song.tracks.iter().filter(|t|!t.notes.is_empty()).map(|t|t.difficulty.id()).collect::<Vec<_>>(),"resolution":song.resolution,"tempo_changes":song.tempos.len(),"time_signatures":song.time_signatures})
        );
        return Ok(());
    }
    let path = Path::new(required(&a, 2)?);
    let store = ModelStore::at(Path::new(required(&a, 3)?).to_path_buf());
    if command == "install-models" {
        for spec in [
            &beatbyte_ml::BEAT_THIS_MEL,
            &beatbyte_ml::BEAT_THIS,
            &beatbyte_ml::BASIC_PITCH,
        ] {
            store.install(spec, &mut |_| {}, &AtomicBool::new(false))?;
        }
        println!("{}", json!({"installed":true}));
        return Ok(());
    }
    let audio = decode_file(path)?;
    if audio.truncated() {
        return Err("audio exceeds 1200 second analysis limit".into());
    }
    if command == "decode" {
        beatbyte_audio::decode::write_wav_mono16(Path::new(required(&a, 4)?), &audio)?;
        println!(
            "{}",
            json!({"duration_s":audio.duration_s(),"sample_rate":audio.sample_rate(),"audio_trim":{"priming_samples":audio.priming().samples,"sample_rate":if audio.priming().timescale == 0 { audio.sample_rate() } else { audio.priming().timescale }}})
        );
    } else if command == "analyze" {
        let mut analysis = SpectralAnalyzer::default().analyze(&audio);
        let mut model = Value::Null;
        if a.get(4).map(String::as_str) == Some("ml") {
            let m =
                beatbyte_meter::track(&Runtime::new(), &store, beatbyte_meter::Size::Full, &audio)?;
            let outcome = beatbyte_meter::apply(&mut analysis, &m, beatbyte_meter::Policy::Grid);
            // This experiment explicitly uses the model's metrical level. A half-time
            // disagreement with the DSP tracker is not a failed model prediction.
            if m.beats.len() < 4 {
                return Err("beat model heard fewer than four beats".into());
            }
            let mut gaps: Vec<f64> = m.beats.windows(2).map(|w| w[1] - w[0]).collect();
            gaps.sort_by(f64::total_cmp);
            let bpm = 60.0 / gaps[gaps.len() / 2];
            if !(20.0..=400.0).contains(&bpm) {
                return Err("model tempo outside supported range".into());
            }
            analysis.beats = m.beats.clone();
            analysis.downbeats = m.downbeats.clone();
            analysis.bpm = bpm;
            analysis.repeats.clear();
            model = json!({"id":m.model,"sha256":m.sha256,"outcome":"Adopted", "policy":"explicit-model-grid-v2", "dsp_comparison":format!("{outcome:?}")});
        }
        println!(
            "{}",
            json!({"analysis":analysis,"meter":model,"audio_trim":{"priming_samples":audio.priming().samples,"sample_rate":if audio.priming().timescale == 0 { audio.sample_rate() } else { audio.priming().timescale }}})
        );
    } else if command == "poly" {
        let t = beatbyte_poly::transcribe(
            &Runtime::new(),
            &store,
            &audio,
            &mut |_, _| {},
            &AtomicBool::new(false),
        )?;
        let notes:Vec<_>=t.notes.iter().map(|n|json!({"time_s":n.start_s,"end_s":n.end_s,"midi":n.midi,"strength":n.amplitude})).collect();
        println!(
            "{}",
            json!({"notes":notes,"model":t.model,"sha256":t.sha256})
        );
    } else if command == "baseline" || command == "study" {
        let mut analysis = SpectralAnalyzer::default().analyze(&audio);
        if command == "study" {
            let mix: Value = serde_json::from_str(&fs::read_to_string(required(&a, 4)?)?)?;
            analysis.beats = serde_json::from_value(mix["analysis"]["beats"].clone())?;
            analysis.downbeats = serde_json::from_value(mix["analysis"]["downbeats"].clone())?;
            analysis.bpm = mix["analysis"]["bpm"].as_f64().ok_or("missing bpm")?;
        }
        let meta = GenerateMeta {
            title: "Baseline".into(),
            artist: "Unknown".into(),
            audio: "song.wav".into(),
        };
        let mut chart = if command == "study" {
            beatbyte_chart::generate::generate_lead_study(&analysis, &meta)
        } else {
            beatbyte_chart::generate_chart(&analysis, &meta)
        };
        chart.audio_trim = Some(beatbyte_chart::AudioTrim {
            priming_samples: audio.priming().samples,
            sample_rate: if audio.priming().timescale == 0 {
                audio.sample_rate()
            } else {
                audio.priming().timescale
            },
        });
        println!("{}", serde_json::to_string(&chart)?);
    } else {
        return Err(format!("unknown command {command}").into());
    }
    Ok(())
}
