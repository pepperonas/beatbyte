//! A menu row as data.
//!
//! A row is a label, what kind of row it is, and the line under it.
//! The kind carries typed accessors into the state it edits — plain
//! function pointers, so a toggle bound to a float does not compile —
//! and everything a list screen does with a row (draw its value, step
//! it, pick its sound) is a pure function here. Nothing in this module
//! knows Bevy; the tests run it on a toy state.
//!
//! Two type parameters keep it generic: `S` is the state the rows edit
//! (`Settings` for the settings screen), `X` names what a row does
//! that is not editing a value — opening another screen, or a custom
//! row that owns its own Enter (the library move, the model download).

/// One row.
pub struct RowSpec<S: 'static, X: 'static> {
    /// What the row is called. Unique within a list; the settings list
    /// is sorted by it.
    pub label: &'static str,
    /// What the row is and how it edits.
    pub kind: Kind<S, X>,
    /// The line shown under the list while the row is selected.
    pub subtitle: Subtitle,
}

/// The line under a selected row.
#[derive(Clone, Copy)]
pub enum Subtitle {
    /// Nothing to explain.
    None,
    /// A fixed sentence.
    Text(&'static str),
    /// A sentence worked out when shown (a path, a backend).
    Live(fn() -> String),
}

/// What a row is.
pub enum Kind<S: 'static, X: 'static> {
    /// On or off. Left and right both flip it.
    Toggle {
        /// Read the value.
        get: fn(&S) -> bool,
        /// Write the value.
        set: fn(&mut S, bool),
        /// How the value reads — [`on_off`] for nearly every toggle.
        words: fn(&S, bool) -> String,
    },
    /// A number stepped between two bounds.
    Slider {
        /// Read the value.
        get: fn(&S) -> f32,
        /// Write the value.
        set: fn(&mut S, f32),
        /// Lowest value.
        min: f32,
        /// Highest value.
        max: f32,
        /// One step.
        step: f32,
        /// How the value reads.
        unit: Unit,
    },
    /// One of a list of named values.
    Choice {
        /// The names, in order (a function, because a list such as
        /// the stage themes is not a constant).
        labels: fn() -> Vec<String>,
        /// The current position in `labels`.
        get: fn(&S) -> usize,
        /// Take the value at a position.
        set: fn(&mut S, usize),
        /// What happens past the last value.
        ends: Ends,
    },
    /// Opens something else; holds no value.
    Door(X),
    /// A row that owns its value and its Enter; the screen handles it
    /// by `id`. `adjust` is what Left / Right do, if anything.
    Custom {
        /// Which row it is, for the screen's handler.
        id: X,
        /// The value shown when the screen has nothing live to say.
        value: fn(&S) -> String,
        /// What a step does.
        adjust: Option<fn(&mut S)>,
        /// Which sound a step makes.
        feel: Feel,
    },
}

/// How a slider's number reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unit {
    /// A share 0..=1 shown as a whole percentage.
    Percent,
    /// Milliseconds with their sign (`+0 ms`, `-25 ms`).
    SignedMs,
    /// Pixels per second.
    PxPerS,
    /// A value held in milliseconds, shown as seconds (`1.25 s`).
    SecondsOfMs,
}

/// What a choice does past its last value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ends {
    /// Goes round (a theme cycle).
    Wrap,
    /// Stops (a size: small, medium, large).
    Stop,
}

/// Which sound a step makes: a switch clicks, a dial ticks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Feel {
    /// A switch: toggles and two-way choices.
    Click,
    /// A dial: sliders and longer choices.
    Tick,
}

/// The words for a toggle.
#[must_use]
pub fn on_off<S>(_: &S, value: bool) -> String {
    if value { "ON" } else { "OFF" }.to_owned()
}

/// A slider's number as it reads. Pure — tested.
#[must_use]
pub fn format_unit(value: f32, unit: Unit) -> String {
    match unit {
        Unit::Percent => format!("{:.0}%", value * 100.0),
        Unit::SignedMs => format!("{value:+.0} ms"),
        Unit::PxPerS => format!("{value:.0} px/s"),
        Unit::SecondsOfMs => format!("{:.2} s", value / 1000.0),
    }
}

impl<S: 'static, X: Copy + 'static> RowSpec<S, X> {
    /// The value as the row shows it. A door shows `OPEN >`.
    #[must_use]
    pub fn value(&self, state: &S) -> String {
        match &self.kind {
            Kind::Toggle { get, words, .. } => words(state, get(state)),
            Kind::Slider { get, unit, .. } => format_unit(get(state), *unit),
            Kind::Choice { labels, get, .. } => {
                let labels = labels();
                labels
                    .get(get(state).min(labels.len().saturating_sub(1)))
                    .cloned()
                    .unwrap_or_default()
            }
            Kind::Door(_) => "OPEN >".to_owned(),
            Kind::Custom { value, .. } => value(state),
        }
    }

    /// One step, `direction` −1 or +1. Returns whether the row is one
    /// that steps (a door and a custom row without `adjust` do not).
    pub fn step(&self, state: &mut S, direction: i32) -> bool {
        match &self.kind {
            Kind::Toggle { get, set, .. } => {
                let flipped = !get(state);
                set(state, flipped);
                true
            }
            Kind::Slider {
                get,
                set,
                min,
                max,
                step,
                ..
            } => {
                let next = (get(state) + step * direction as f32).clamp(*min, *max);
                set(state, next);
                true
            }
            Kind::Choice {
                labels,
                get,
                set,
                ends,
            } => {
                let count = labels().len();
                if count == 0 {
                    return false;
                }
                set(state, next_index(get(state), count, direction, *ends));
                true
            }
            Kind::Door(_) => false,
            Kind::Custom { adjust, .. } => adjust.is_some_and(|adjust| {
                adjust(state);
                true
            }),
        }
    }

    /// Whether the row's value differs from `default`'s. Only a row
    /// that holds a value has a default: a door and a custom row never
    /// differ. Pure — tested.
    #[must_use]
    pub fn differs(&self, state: &S, default: &S) -> bool {
        match &self.kind {
            Kind::Toggle { get, .. } => get(state) != get(default),
            Kind::Slider { get, step, .. } => (get(state) - get(default)).abs() > step * 0.5,
            Kind::Choice { get, .. } => get(state) != get(default),
            Kind::Door(_) | Kind::Custom { .. } => false,
        }
    }

    /// Put the row back to `default`'s value. Returns whether anything
    /// changed. Pure — tested.
    pub fn reset(&self, state: &mut S, default: &S) -> bool {
        if !self.differs(state, default) {
            return false;
        }
        match &self.kind {
            Kind::Toggle { get, set, .. } => set(state, get(default)),
            Kind::Slider { get, set, .. } => set(state, get(default)),
            Kind::Choice { get, set, .. } => set(state, get(default)),
            Kind::Door(_) | Kind::Custom { .. } => return false,
        }
        true
    }

    /// Which sound a step makes.
    #[must_use]
    pub fn feel(&self) -> Feel {
        match &self.kind {
            Kind::Toggle { .. } | Kind::Door(_) => Feel::Click,
            Kind::Slider { .. } => Feel::Tick,
            Kind::Choice { labels, .. } if labels().len() <= 2 => Feel::Click,
            Kind::Choice { .. } => Feel::Tick,
            Kind::Custom { feel, .. } => *feel,
        }
    }

    /// The screen a door opens, or a custom row's id.
    #[must_use]
    pub fn action(&self) -> Option<X> {
        match &self.kind {
            Kind::Door(x) | Kind::Custom { id: x, .. } => Some(*x),
            _ => None,
        }
    }

    /// Whether the row is a door.
    #[must_use]
    pub fn is_door(&self) -> bool {
        matches!(self.kind, Kind::Door(_))
    }

    /// The line under the row when selected.
    #[must_use]
    pub fn subtitle(&self) -> String {
        match self.subtitle {
            Subtitle::None => String::new(),
            Subtitle::Text(text) => text.to_owned(),
            Subtitle::Live(live) => live(),
        }
    }
}

/// The position after one step through `count` values. Pure — tested.
#[must_use]
pub fn next_index(current: usize, count: usize, direction: i32, ends: Ends) -> usize {
    if count == 0 {
        return 0;
    }
    let current = current.min(count - 1) as i64;
    let next = current + i64::from(direction.signum());
    match ends {
        Ends::Wrap => next.rem_euclid(count as i64) as usize,
        Ends::Stop => next.clamp(0, count as i64 - 1) as usize,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Default, PartialEq)]
    struct Toy {
        on: bool,
        level: f32,
        size: usize,
        cleared: bool,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Act {
        Open,
        Mine,
    }

    fn sizes() -> Vec<String> {
        ["SMALL", "MEDIUM", "LARGE"].map(str::to_owned).to_vec()
    }

    const TOGGLE: RowSpec<Toy, Act> = RowSpec {
        label: "TOGGLE",
        kind: Kind::Toggle {
            get: |t| t.on,
            set: |t, v| t.on = v,
            words: on_off,
        },
        subtitle: Subtitle::None,
    };
    const SLIDER: RowSpec<Toy, Act> = RowSpec {
        label: "SLIDER",
        kind: Kind::Slider {
            get: |t| t.level,
            set: |t, v| t.level = v,
            min: 0.0,
            max: 1.0,
            step: 0.25,
            unit: Unit::Percent,
        },
        subtitle: Subtitle::Text("a share"),
    };
    const SIZE: RowSpec<Toy, Act> = RowSpec {
        label: "SIZE",
        kind: Kind::Choice {
            labels: sizes,
            get: |t| t.size,
            set: |t, i| t.size = i,
            ends: Ends::Stop,
        },
        subtitle: Subtitle::None,
    };
    const DOOR: RowSpec<Toy, Act> = RowSpec {
        label: "DOOR",
        kind: Kind::Door(Act::Open),
        subtitle: Subtitle::None,
    };
    const MINE: RowSpec<Toy, Act> = RowSpec {
        label: "MINE",
        kind: Kind::Custom {
            id: Act::Mine,
            value: |_| "LIVE".to_owned(),
            adjust: Some(|t| t.cleared = true),
            feel: Feel::Click,
        },
        subtitle: Subtitle::Live(|| "worked out".to_owned()),
    };

    /// Every row that holds a value knows whether it left its default
    /// and goes back to it; a door and a custom row have no default.
    #[test]
    fn a_value_row_resets_to_its_default_and_a_door_has_none() {
        let default = Toy::default();
        let mut toy = Toy::default();
        for row in [&TOGGLE, &SLIDER, &SIZE] {
            assert!(
                !row.differs(&toy, &default),
                "{} starts at its default",
                row.label
            );
            assert!(row.step(&mut toy, 1));
            assert!(row.differs(&toy, &default), "{} moved", row.label);
            assert!(row.reset(&mut toy, &default));
            assert!(!row.differs(&toy, &default), "{} is back", row.label);
            assert!(
                !row.reset(&mut toy, &default),
                "a second reset changes nothing"
            );
        }
        assert_eq!(toy, default);
        for row in [&DOOR, &MINE] {
            assert!(!row.differs(&toy, &default));
            assert!(!row.reset(&mut toy, &default));
        }
    }

    #[test]
    fn a_toggle_flips_either_way_and_reads_on_off() {
        let mut toy = Toy::default();
        assert_eq!(TOGGLE.value(&toy), "OFF");
        assert!(TOGGLE.step(&mut toy, -1));
        assert_eq!(TOGGLE.value(&toy), "ON");
        assert!(TOGGLE.step(&mut toy, 1));
        assert!(!toy.on);
        assert_eq!(TOGGLE.feel(), Feel::Click);
    }

    #[test]
    fn a_slider_steps_and_stays_inside_its_bounds() {
        let mut toy = Toy::default();
        SLIDER.step(&mut toy, -1);
        assert!(toy.level.abs() < f32::EPSILON, "clamped at the bottom");
        for _ in 0..10 {
            SLIDER.step(&mut toy, 1);
        }
        assert!((toy.level - 1.0).abs() < f32::EPSILON, "clamped at the top");
        assert_eq!(SLIDER.value(&toy), "100%");
        SLIDER.step(&mut toy, -1);
        assert_eq!(SLIDER.value(&toy), "75%");
        assert_eq!(SLIDER.feel(), Feel::Tick);
        assert_eq!(SLIDER.subtitle(), "a share");
    }

    #[test]
    fn every_unit_reads_as_it_did() {
        assert_eq!(format_unit(0.8, Unit::Percent), "80%");
        assert_eq!(format_unit(0.0, Unit::SignedMs), "+0 ms");
        assert_eq!(format_unit(-25.0, Unit::SignedMs), "-25 ms");
        assert_eq!(format_unit(480.0, Unit::PxPerS), "480 px/s");
        assert_eq!(format_unit(1250.0, Unit::SecondsOfMs), "1.25 s");
    }

    #[test]
    fn a_choice_stops_or_wraps_as_it_says() {
        let mut toy = Toy::default();
        SIZE.step(&mut toy, -1);
        assert_eq!(SIZE.value(&toy), "SMALL", "a stopping choice stops");
        SIZE.step(&mut toy, 1);
        SIZE.step(&mut toy, 1);
        SIZE.step(&mut toy, 1);
        assert_eq!(SIZE.value(&toy), "LARGE");
        assert_eq!(SIZE.feel(), Feel::Tick, "three values tick");
        assert_eq!(next_index(2, 3, 1, Ends::Wrap), 0);
        assert_eq!(next_index(0, 3, -1, Ends::Wrap), 2);
        assert_eq!(next_index(0, 3, -1, Ends::Stop), 0);
        assert_eq!(
            next_index(9, 3, 1, Ends::Stop),
            2,
            "an out-of-range value stays inside"
        );
        assert_eq!(next_index(0, 0, 1, Ends::Wrap), 0);
        // An out-of-range position reads as the last value, never panics.
        toy.size = 7;
        assert_eq!(SIZE.value(&toy), "LARGE");
    }

    #[test]
    fn a_two_way_choice_clicks_like_a_switch() {
        fn two() -> Vec<String> {
            vec!["A".to_owned(), "B".to_owned()]
        }
        let pair: RowSpec<Toy, Act> = RowSpec {
            label: "PAIR",
            kind: Kind::Choice {
                labels: two,
                get: |t| t.size,
                set: |t, i| t.size = i,
                ends: Ends::Wrap,
            },
            subtitle: Subtitle::None,
        };
        assert_eq!(pair.feel(), Feel::Click);
    }

    #[test]
    fn a_door_and_a_custom_row_say_what_they_do() {
        let mut toy = Toy::default();
        assert_eq!(DOOR.value(&toy), "OPEN >");
        assert!(!DOOR.step(&mut toy, 1), "a door holds no value");
        assert_eq!(DOOR.action(), Some(Act::Open));
        assert!(DOOR.is_door());
        assert_eq!(MINE.value(&toy), "LIVE");
        assert_eq!(MINE.action(), Some(Act::Mine));
        assert!(!MINE.is_door());
        assert!(MINE.step(&mut toy, -1));
        assert!(toy.cleared);
        assert_eq!(MINE.subtitle(), "worked out");
        assert_eq!(TOGGLE.action(), None);
    }
}
