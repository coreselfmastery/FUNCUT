// SPDX-License-Identifier: AGPL-3.0-or-later
// FileCopyrightText: 2026 Jareer Abdullah and Concat contributors

//! The grade: colour grading inside the comp, as CSS the engine draws.
//!
//! A LUT is a lookup table the footage passes through, and a comp cannot
//! carry one - but it can carry the CSS that approximates it: a
//! full-bleed box, first in the comp's paint order, whose
//! `backdrop-filter` (`saturate()`, `hue-rotate()`, `contrast()`,
//! `brightness()`) does the tonal work and the hue turn, and whose
//! translucent tint, blended `soft-light`, does the cast. What the
//! engine has under that box - the scene's plate, the layers beneath -
//! is what gets graded; the comp's own words stay as inked, which is
//! the whole point of grading inside the comp rather than over the
//! render.
//!
//! The temperatures are the seven the emotion read names for a line
//! (`cold`, `warm`, `cool`, `hot`, `sickly`, `split`, `balanced`), so
//! the act a line sits in is the grade its scene wears: a cold hook
//! opening into a warm payoff is the arc, in two fields. A brand's own
//! colour rides in `tint`, and the grade keeps the palette by wearing it
//! instead of the stock cast.
//!
//! ```text
//! {"temperature": "warm", "amount": 0.8}
//!   -> backdrop-filter: saturate(1.12) contrast(1.04) brightness(1.02)
//!      background: rgba(255,178,92,0.18); mix-blend-mode: soft-light
//! ```
//!
//! Measured against the engine on a neutral grey plate, the warm grade
//! at full amount moves the frame +40 red and -10 blue and leaves the
//! comp's own text untouched: the cast is real, the ink is safe.

use serde::{Deserialize, Serialize};

use crate::comp::{Blend, Comp, Length, Node, NodeKind};

/// The seven temperatures the emotion read names, as the tuning a scene's
/// grade starts from. Each fixes the tonal moves and the cast;
/// [`Grade`]'s own fields override and `amount` scales the cast.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Temperature {
    /// Pushed contrast, an amber-red bias, crushed blacks: the line that
    /// lands like a hit.
    Hot,
    /// A green-yellow cast over low saturation: the wrong kind of quiet.
    Sickly,
    /// A teal-blue bias, crushed blacks, low key: the cold open, the
    /// fact before the feeling.
    Cold,
    /// Golden hour: lifted blacks, saturated highs: the payoff.
    Warm,
    /// No cast and no tonal move - the brand guide's base grade, given
    /// back as nothing until the explicit fields say otherwise.
    Balanced,
    /// Desaturated blue-grey with a soft roll-off: the steady middle.
    Cool,
    /// Teal in the shadows, orange in the highs - the classic
    /// two-temperature grade, approximated as a top-to-bottom gradient
    /// of the two casts, which reads as split the moment anything moves
    /// under it.
    Split,
}

/// One tuning: the tonal moves, the cast colour and how hard it is put
/// on. Every temperature's row in the table above, in numbers.
struct Tuning {
    saturate: f64,
    contrast: f64,
    brightness: f64,
    /// The hue move, in degrees of `hue-rotate()`: 0 is none. The cast
    /// does most of a temperature's hue work; this is the move that
    /// turns the whole picture's palette rather than laying a colour
    /// over it - the LUT approximation's third leg, for the
    /// temperatures whose shift is not one colour deep.
    hue: f64,
    /// The cast, as 8-bit RGB and an alpha. `None` is no cast at all.
    cast: Option<([u8; 3], f64)>,
    /// The second cast of a split grade; ignored unless `cast` is some.
    counter: Option<[u8; 3]>,
}

fn tuning(temperature: Temperature) -> Tuning {
    match temperature {
        Temperature::Hot => Tuning {
            saturate: 1.20,
            contrast: 1.12,
            brightness: 0.98,
            hue: 0.0,
            cast: Some(([255, 120, 60], 0.26)),
            counter: None,
        },
        Temperature::Sickly => Tuning {
            saturate: 0.80,
            contrast: 1.06,
            brightness: 0.97,
            // The whole palette leans green-yellow, not just the cast
            // over it: the wrong kind of quiet is a wheel turned.
            hue: 25.0,
            cast: Some(([150, 160, 60], 0.20)),
            counter: None,
        },
        Temperature::Cold => Tuning {
            saturate: 0.88,
            contrast: 1.10,
            brightness: 0.94,
            hue: 0.0,
            cast: Some(([24, 68, 102], 0.28)),
            counter: None,
        },
        Temperature::Warm => Tuning {
            saturate: 1.12,
            contrast: 1.04,
            brightness: 1.02,
            hue: 0.0,
            cast: Some(([255, 178, 92], 0.22)),
            counter: None,
        },
        Temperature::Balanced => Tuning {
            saturate: 1.0,
            contrast: 1.0,
            brightness: 1.0,
            hue: 0.0,
            cast: None,
            counter: None,
        },
        Temperature::Cool => Tuning {
            saturate: 0.92,
            contrast: 1.03,
            brightness: 0.99,
            hue: 0.0,
            cast: Some(([96, 128, 168], 0.22)),
            counter: None,
        },
        Temperature::Split => Tuning {
            saturate: 1.15,
            contrast: 1.07,
            brightness: 1.0,
            // The split's own small wheel turn: cools deepen a touch so
            // the orange highs read warmer against them.
            hue: -5.0,
            cast: Some(([18, 86, 120], 0.24)),
            counter: Some([255, 150, 60]),
        },
    }
}

/// A scene's colour grade, as a storyboard writes it on the scene:
/// `{"temperature": "warm", "amount": 0.8}`. The temperature picks the
/// tuning; `amount` scales the cast; the three tonal fields and `tint`
/// override. All of it lands in the comp as one full-bleed box under
/// everything the comp itself paints.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct Grade {
    /// The tuning this grade starts from. Defaults to `balanced` - the
    /// base grade - so `{"saturate": 1.2}` alone is a neutral tonal move.
    #[serde(default = "balanced")]
    pub temperature: Temperature,
    /// How much of the cast is put on, 0 to 1; the tonal moves are not
    /// scaled by it. Defaults to 1 - the temperature's own strength.
    #[serde(default = "full")]
    pub amount: f64,
    /// `saturate()` over what is under the box, 1 untouched. Overrides
    /// the temperature's move.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub saturate: Option<f64>,
    /// `contrast()`, 1 untouched.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contrast: Option<f64>,
    /// `brightness()`, 1 untouched.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub brightness: Option<f64>,
    /// `hue-rotate()`, in degrees: -360 to 360, 0 untouched. The move
    /// that turns the palette itself rather than laying a cast over
    /// it - the third leg of the LUT approximation beside `saturate()`
    /// and `contrast()`. Overrides the temperature's own turn.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hue: Option<f64>,
    /// The brand's own colour as the cast, `#rrggbb` or `#rrggbbaa`,
    /// worn instead of the temperature's stock cast. On a split grade it
    /// replaces the gradient with the one colour.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tint: Option<String>,
}

fn balanced() -> Temperature {
    Temperature::Balanced
}

fn full() -> f64 {
    1.0
}

impl Grade {
    /// A grade at its temperature's own strength: the shorthand for a
    /// builder that has no call to tune anything.
    pub fn at(temperature: Temperature) -> Grade {
        Grade {
            temperature,
            amount: 1.0,
            saturate: None,
            contrast: None,
            brightness: None,
            hue: None,
            tint: None,
        }
    }

    /// Everything wrong with this grade, in words a storyboard can put
    /// beside its scene's number. Empty is ready to bake.
    pub fn problems(&self) -> Vec<String> {
        let mut problems = Vec::new();
        let name = format!("{:?}", self.temperature).to_lowercase();
        if !self.amount.is_finite() || !(0.0..=1.0).contains(&self.amount) {
            problems.push(format!(
                "the grade's `amount` is {}, and it is 0 to 1",
                number(self.amount)
            ));
        }
        for (field, value) in [
            ("saturate", self.saturate),
            ("contrast", self.contrast),
            ("brightness", self.brightness),
        ] {
            if let Some(value) = value
                && (!value.is_finite() || !(0.1..=4.0).contains(&value))
            {
                problems.push(format!(
                    "the grade's `{field}` is {}, and it is between 0.1 and 4",
                    number(value)
                ));
            }
        }
        if let Some(hue) = self.hue
            && (!hue.is_finite() || !(-360.0..=360.0).contains(&hue))
        {
            problems.push(format!(
                "the grade's `hue` is {}, and it is between -360 and 360 degrees",
                number(hue)
            ));
        }
        if let Some(tint) = &self.tint {
            if parse_tint(tint).is_none() {
                problems.push(format!(
                    "the grade's `tint` is {tint:?}, and a colour is #rrggbb or #rrggbbaa"
                ));
            } else if self.temperature == Temperature::Balanced {
                problems.push(format!(
                    "the {name} grade carries a `tint`, and a balanced one has no cast to wear it"
                ));
            }
        }
        problems
    }

    /// The stylesheet the grade box is drawn with: one rule, the whole
    /// grade. Identity moves are left out - `saturate(1)` says nothing -
    /// and a grade that says nothing at all says nothing at all.
    fn stylesheet(&self) -> String {
        let tuning = tuning(self.temperature);
        let saturate = self.saturate.unwrap_or(tuning.saturate);
        let contrast = self.contrast.unwrap_or(tuning.contrast);
        let brightness = self.brightness.unwrap_or(tuning.brightness);
        let hue = self.hue.unwrap_or(tuning.hue);
        let mut moves = String::new();
        // The LUT approximation's three legs, in the order a colourist
        // thinks them: the saturation, the hue turn, the contrast, the
        // level.
        for (value, name) in [
            (saturate, "saturate"),
            (hue, "hue-rotate"),
            (contrast, "contrast"),
            (brightness, "brightness"),
        ] {
            let said = if name == "hue-rotate" {
                format!("{name}({}deg)", number(value))
            } else {
                format!("{name}({})", number(value))
            };
            if (value - if name == "hue-rotate" { 0.0 } else { 1.0 }).abs() > 1e-9 {
                if !moves.is_empty() {
                    moves.push(' ');
                }
                moves.push_str(&said);
            }
        }
        // The cast, as the engine wears it: the temperature's own colour
        // or the brand's tint, with the grade's `amount` and a tint's own
        // alpha already folded in - one number, ready for the rule.
        let cast: Option<([u8; 3], f64)> = match (&self.tint, tuning.cast) {
            (Some(tint), Some((_, alpha))) => {
                parse_tint(tint).map(|([r, g, b], own)| ([r, g, b], alpha * own * self.amount))
            }
            // A tint on a balanced grade is refused by `problems`; the
            // quarter is only so this stays total.
            (Some(tint), None) => {
                parse_tint(tint).map(|([r, g, b], own)| ([r, g, b], 0.25 * own * self.amount))
            }
            (None, Some(([r, g, b], alpha))) => Some(([r, g, b], alpha * self.amount)),
            (None, None) => None,
        };
        let background = match (cast, tuning.counter, self.tint.as_deref()) {
            // The split wears its two casts top to bottom - unless the
            // brand's own colour came in and replaced the pair.
            (Some(([r, g, b], a)), Some([r2, g2, b2]), None) if a > 1e-9 => {
                format!(
                    "background:linear-gradient(180deg,rgba({r},{g},{b},{}),rgba({r2},{g2},{b2},{}));",
                    number(a),
                    number(a)
                )
            }
            (Some(([r, g, b], a)), _, _) if a > 1e-9 => {
                format!("background:rgba({r},{g},{b},{});", number(a))
            }
            _ => String::new(),
        };
        let blend = if background.is_empty() {
            ""
        } else {
            "mix-blend-mode:soft-light;"
        };
        if moves.is_empty() && background.is_empty() {
            // A grade with nothing to say still draws an empty box: the
            // caller checks `is_identity` first, this is the safety net.
            return String::new();
        }
        format!(
            ".funcut-grade{{position:absolute;left:0;top:0;width:100%;height:100%;\
             backdrop-filter:{moves};{background}{blend}}}"
        )
    }

    /// Whether the grade, as written, changes anything: a balanced grade
    /// with no overrides and no cast draws nothing, and baking it would
    /// only add a box the engine has to carry.
    pub fn is_identity(&self) -> bool {
        let tuning = tuning(self.temperature);
        let untouched = |value: f64| (value - 1.0).abs() <= 1e-9;
        let no_turn = |value: f64| value.abs() <= 1e-9;
        untouched(self.saturate.unwrap_or(tuning.saturate))
            && untouched(self.contrast.unwrap_or(tuning.contrast))
            && untouched(self.brightness.unwrap_or(tuning.brightness))
            && no_turn(self.hue.unwrap_or(tuning.hue))
            && (tuning.cast.is_none() || self.amount <= 1e-9)
            && self.tint.is_none()
    }
}

/// A colour as `#rrggbb` or `#rrggbbaa`, case-insensitive: the brand
/// palette's own string. Returns 8-bit RGB and an alpha that has already
/// eaten the grade's `amount`, so the caller never sees a raw one.
fn parse_tint(text: &str) -> Option<([u8; 3], f64)> {
    let text = text.trim();
    let hex = text.strip_prefix('#')?;
    let channel = |at: usize| u8::from_str_radix(hex.get(at..at + 2)?, 16).ok();
    match hex.len() {
        6 => Some(([channel(0)?, channel(2)?, channel(4)?], 1.0)),
        8 => {
            let alpha = u16::from_str_radix(hex.get(6..8)?, 16).ok()?;
            Some((
                [channel(0)?, channel(2)?, channel(4)?],
                alpha as f64 / 255.0,
            ))
        }
        _ => None,
    }
}

/// A number as CSS wants it: two decimals at most, so `1.12` never
/// becomes `1.1200000000001`.
fn number(value: f64) -> String {
    let trimmed = (value * 100.0).round() / 100.0;
    if (trimmed - trimmed.round()).abs() < 1e-9 {
        format!("{}", trimmed.round() as i64)
    } else {
        format!("{trimmed}")
    }
}

/// Bake a grade into a comp: one node, first in paint order, so the
/// engine draws it under everything the comp paints and over whatever
/// the scene has under the comp - the plate, the lower layers. The
/// comp's own content is never touched, and neither is anything the
/// scene layers above the comp.
///
/// An identity grade bakes nothing: a box that grades nothing is a box
/// the engine would have to carry for no picture. A grade that is wrong
/// says so in its own words, for the caller to name the scene.
pub fn apply(comp: &mut Comp, grade: &Grade) -> Result<(), String> {
    let problems = grade.problems();
    if let Some(problem) = problems.first() {
        return Err(problem.clone());
    }
    if grade.is_identity() {
        return Ok(());
    }
    let stylesheet = grade.stylesheet();
    debug_assert!(
        !stylesheet.is_empty(),
        "a non-identity grade has a stylesheet"
    );
    if stylesheet.is_empty() {
        return Ok(());
    }
    // The grade node's name, unique in this comp: a second grade on a
    // nested composition is not a collision to refuse, it is a name to
    // have.
    let mut id = "funcut-grade".to_owned();
    let mut suffix = 1;
    while comp.nodes.iter().any(|node| node.id == id) {
        suffix += 1;
        id = format!("funcut-grade-{suffix}");
    }
    let node = Node {
        id,
        parent: None,
        // The whole comp's span: `duration: 0` runs to the comp's end,
        // which the sequence clamps to the scene's hold.
        start: 0.0,
        duration: 0.0,
        transform: Default::default(),
        opacity: 1.0,
        blend: Blend::Normal,
        mask: None,
        effects: Vec::new(),
        tracks: Default::default(),
        kind: NodeKind::Html {
            markup: r#"<div class="funcut-grade"></div>"#.to_owned(),
            css: stylesheet,
            // A percentage box: the frame, whatever size the reel is,
            // because a grade covers the picture, not the comp's
            // authored sheet of paper.
            width: Length::Percent(100.0),
            height: Length::Percent(100.0),
        },
    };
    comp.nodes.insert(0, node);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::comp::{Comp, FORMAT, Rgba};
    use std::collections::BTreeMap;

    fn empty_comp() -> Comp {
        Comp {
            format: FORMAT,
            id: "test.grade".into(),
            name: "Grade".into(),
            width: 1920,
            height: 1080,
            fps: 30.0,
            duration: 4.0,
            background: Rgba::new(0.0, 0.0, 0.0, 0.0),
            assets: BTreeMap::new(),
            curves: BTreeMap::new(),
            nodes: Vec::new(),
            slots: Vec::new(),
        }
    }

    fn grade_from(text: &str) -> Grade {
        serde_json::from_str(text).expect("the grade parses")
    }

    #[test]
    fn the_temperature_names_are_the_emotion_reads_own() {
        for name in ["hot", "sickly", "cold", "warm", "balanced", "cool", "split"] {
            let grade: Grade = serde_json::from_str(&format!("{{\"temperature\": \"{name}\"}}"))
                .unwrap_or_else(|error| panic!("{name}: {error}"));
            assert_eq!(
                serde_json::to_value(grade.temperature)
                    .expect("serializes")
                    .as_str(),
                Some(name)
            );
        }
        // A name the read never says is refused at the door, in serde's
        // own words, naming every one it does.
        assert!(serde_json::from_str::<Grade>("{\"temperature\": \"chilly\"}").is_err());
    }

    #[test]
    fn the_defaults_are_the_temperature_and_the_full_amount() {
        let grade = grade_from(r#"{"temperature": "warm"}"#);
        assert_eq!(grade.temperature, Temperature::Warm);
        assert_eq!(grade.amount, 1.0);
        assert_eq!(grade.saturate, None);
        // And a grade with no temperature at all is the balanced base.
        let base = grade_from(r#"{"saturate": 1.2}"#);
        assert_eq!(base.temperature, Temperature::Balanced);
    }

    #[test]
    fn a_grade_is_read_from_its_string_and_written_back() {
        let text = r##"{"temperature":"split","amount":0.7,"saturate":1.3,"tint":"#f4b860"}"##;
        let grade = grade_from(text);
        assert_eq!(grade.temperature, Temperature::Split);
        assert_eq!(grade.amount, 0.7);
        assert_eq!(grade.saturate, Some(1.3));
        assert_eq!(grade.tint.as_deref(), Some("#f4b860"));
        // The written form is the reading form: a storyboard saved is a
        // storyboard loaded.
        let written = serde_json::to_string(&grade).expect("writes");
        let read_back: Grade = serde_json::from_str(&written).expect("reads back");
        assert_eq!(read_back, grade);
    }

    #[test]
    fn the_stylesheet_is_one_rule_with_the_tonal_moves_and_the_cast() {
        let css = grade_from(r#"{"temperature": "warm"}"#).stylesheet();
        assert!(css.contains(".funcut-grade{"), "{css}");
        assert!(
            css.contains("backdrop-filter:saturate(1.12) contrast(1.04) brightness(1.02);"),
            "{css}"
        );
        assert!(css.contains("background:rgba(255,178,92,0.22);"), "{css}");
        assert!(css.contains("mix-blend-mode:soft-light;"), "{css}");
        assert!(
            css.contains("width:100%;height:100%"),
            "the box is the whole frame: {css}"
        );
    }

    #[test]
    fn the_amount_scales_the_cast_and_not_the_tonal_moves() {
        let css = grade_from(r#"{"temperature": "warm", "amount": 0.5}"#).stylesheet();
        assert!(
            css.contains("backdrop-filter:saturate(1.12) contrast(1.04) brightness(1.02);"),
            "{css}"
        );
        assert!(css.contains("background:rgba(255,178,92,0.11);"), "{css}");
        // And nothing at all is no cast at all.
        let none = grade_from(r#"{"temperature": "warm", "amount": 0}"#).stylesheet();
        assert!(!none.contains("background:"), "{none}");
        assert!(!none.contains("soft-light"), "{none}");
    }

    #[test]
    fn a_sickly_grade_turns_the_whole_wheel_not_just_the_cast() {
        // The wrong kind of quiet is a palette turned green-yellow:
        // `hue-rotate()` between the saturation and the contrast, the
        // LUT approximation's third leg.
        let css = grade_from(r#"{"temperature": "sickly"}"#).stylesheet();
        assert!(
            css.contains(
                "backdrop-filter:saturate(0.8) hue-rotate(25deg) contrast(1.06) brightness(0.97);"
            ),
            "{css}"
        );
        // The split turns the other way, a touch, so its orange highs
        // read warmer against deepened cools.
        let split = grade_from(r#"{"temperature": "split"}"#).stylesheet();
        assert!(split.contains("hue-rotate(-5deg)"), "{split}");
    }

    #[test]
    fn a_hue_override_replaces_the_temperatures_own_turn() {
        let css = grade_from(r#"{"temperature": "cold", "hue": -20}"#).stylesheet();
        assert!(css.contains("hue-rotate(-20deg)"), "{css}");
        // A zero override is no turn at all, even on a temperature that
        // carries one: the override wins both ways.
        let flat = grade_from(r#"{"temperature": "sickly", "hue": 0}"#).stylesheet();
        assert!(!flat.contains("hue-rotate"), "{flat}");
        // And a hue alone on the balanced base is a grade - a neutral
        // wheel turn asked for by name.
        let turn = grade_from(r#"{"hue": 12}"#);
        assert!(!turn.is_identity());
        assert!(turn.stylesheet().contains("hue-rotate(12deg)"));
        // The reading form is the writing form.
        let written = serde_json::to_string(&turn).expect("writes");
        let read_back: Grade = serde_json::from_str(&written).expect("reads back");
        assert_eq!(read_back, turn);
    }

    #[test]
    fn a_hue_beyond_the_wheel_is_named_as_the_mistake_it_is() {
        let problems = grade_from(r#"{"temperature": "warm", "hue": 400}"#).problems();
        assert!(
            problems.iter().any(|p| p.contains("`hue` is 400")),
            "{problems:?}"
        );
        let mut comp = empty_comp();
        assert!(apply(&mut comp, &grade_from(r#"{"hue": 720}"#)).is_err());
    }

    #[test]
    fn the_overrides_replace_the_temperatures_own_moves() {
        let css = grade_from(r#"{"temperature": "cold", "saturate": 1.0, "brightness": 1.05}"#)
            .stylesheet();
        assert!(
            css.contains("backdrop-filter:contrast(1.1) brightness(1.05);"),
            "{css}"
        );
        assert!(!css.contains("saturate"), "{css}");
    }

    #[test]
    fn a_split_is_two_casts_and_a_solid_tint_is_one() {
        let split = grade_from(r#"{"temperature": "split"}"#).stylesheet();
        assert!(
            split.contains("linear-gradient(180deg,rgba(18,86,120,0.24),rgba(255,150,60,0.24))"),
            "{split}"
        );
        let solid = grade_from(r##"{"temperature": "split", "tint": "#3165a4"}"##).stylesheet();
        assert!(
            solid.contains("background:rgba(49,101,164,0.24);"),
            "{solid}"
        );
        assert!(!solid.contains("linear-gradient"), "{solid}");
    }

    #[test]
    fn a_tint_with_alpha_of_its_own_wears_that_alpha() {
        let css = grade_from(r##"{"temperature": "cool", "tint": "#96a8d080"}"##).stylesheet();
        // The stock cast's strength times the tint's own half.
        assert!(css.contains("background:rgba(150,168,208,0.11);"), "{css}");
    }

    #[test]
    fn a_balanced_grade_with_no_overrides_is_the_identity_and_bakes_nothing() {
        let identity = grade_from(r#"{"temperature": "balanced"}"#);
        assert!(identity.is_identity());
        let mut comp = empty_comp();
        assert_eq!(apply(&mut comp, &identity), Ok(()));
        assert!(comp.nodes.is_empty(), "no box for nothing");
        // A real grade is not it.
        assert!(!grade_from(r#"{"temperature": "cold"}"#).is_identity());
        // An explicit move on a balanced temperature is a grade: the
        // neutral tonal move is a thing to ask for by name.
        assert!(!grade_from(r#"{"saturate": 1.2}"#).is_identity());
        // And a cast at zero strength still carries the tonal moves -
        // the amount scales the cast and never the moves.
        assert!(!grade_from(r#"{"temperature": "warm", "amount": 0}"#).is_identity());
    }

    #[test]
    fn the_grade_bakes_in_first_and_spans_the_whole_comp() {
        let mut comp = empty_comp();
        let existing = Node {
            id: "mark".into(),
            parent: None,
            start: 0.0,
            duration: 0.0,
            transform: Default::default(),
            opacity: 1.0,
            blend: Blend::Normal,
            mask: None,
            effects: Vec::new(),
            tracks: BTreeMap::new(),
            kind: NodeKind::Html {
                markup: "<div>wordmark</div>".into(),
                css: String::new(),
                width: Length::Px(1920.0),
                height: Length::Px(1080.0),
            },
        };
        comp.nodes.push(existing);
        apply(&mut comp, &grade_from(r#"{"temperature": "cold"}"#)).expect("bakes");
        assert_eq!(comp.nodes.len(), 2);
        let grade = &comp.nodes[0];
        assert_eq!(grade.id, "funcut-grade");
        assert_eq!(grade.start, 0.0);
        assert_eq!(grade.duration, 0.0, "zero runs to the comp's end");
        let NodeKind::Html {
            markup,
            css,
            width,
            height,
        } = &grade.kind
        else {
            panic!("the grade is a markup node");
        };
        assert_eq!(markup, r#"<div class="funcut-grade"></div>"#);
        assert!(css.contains("backdrop-filter:"), "{css}");
        assert_eq!(*width, Length::Percent(100.0));
        assert_eq!(*height, Length::Percent(100.0));
        // The comp's own node is untouched above it.
        assert_eq!(comp.nodes[1].id, "mark");
    }

    #[test]
    fn a_second_grade_node_gets_its_own_name_rather_than_a_collision() {
        let mut comp = empty_comp();
        let first = Node {
            id: "funcut-grade".into(),
            parent: None,
            start: 0.0,
            duration: 0.0,
            transform: Default::default(),
            opacity: 1.0,
            blend: Blend::Normal,
            mask: None,
            effects: Vec::new(),
            tracks: BTreeMap::new(),
            kind: NodeKind::Html {
                markup: String::new(),
                css: String::new(),
                width: Length::Percent(100.0),
                height: Length::Percent(100.0),
            },
        };
        comp.nodes.push(first);
        apply(&mut comp, &grade_from(r#"{"temperature": "warm"}"#)).expect("bakes");
        assert_eq!(comp.nodes[0].id, "funcut-grade-2");
    }

    #[test]
    fn a_bad_grade_says_what_is_wrong_with_it() {
        let problems = grade_from(r#"{"temperature": "warm", "amount": 1.4}"#).problems();
        assert!(
            problems.iter().any(|p| p.contains("`amount` is 1.4")),
            "{problems:?}"
        );
        let problems = grade_from(r#"{"temperature": "warm", "tint": "orange"}"#).problems();
        assert!(
            problems.iter().any(|p| p.contains("`tint` is \"orange\"")),
            "{problems:?}"
        );
        let problems = grade_from(r#"{"temperature": "warm", "contrast": 9.0}"#).problems();
        assert!(
            problems.iter().any(|p| p.contains("`contrast` is 9")),
            "{problems:?}"
        );
        // And the refusal carries into the bake.
        let mut comp = empty_comp();
        assert!(apply(&mut comp, &grade_from(r#"{"amount": -0.2}"#)).is_err());
    }

    #[test]
    fn a_tint_on_a_balanced_grade_is_named_as_the_mistake_it_is() {
        let problems = grade_from(r##"{"tint": "#ff8800"}"##).problems();
        assert!(
            problems
                .iter()
                .any(|p| p.contains("a balanced one has no cast to wear it")),
            "{problems:?}"
        );
    }

    #[test]
    fn numbers_are_css_numbers_not_rust_ones() {
        assert_eq!(number(1.12), "1.12");
        assert_eq!(number(1.0), "1");
        assert_eq!(number(0.9499999999999999), "0.95");
        assert_eq!(number(4.0), "4");
    }

    #[test]
    fn a_tint_is_parsed_as_six_or_eight_hex_digits() {
        assert_eq!(parse_tint("#f4b860"), Some(([244, 184, 96], 1.0)));
        assert_eq!(parse_tint("#F4B860"), Some(([244, 184, 96], 1.0)));
        assert_eq!(
            parse_tint("#f4b86080"),
            Some(([244, 184, 96], 128.0 / 255.0))
        );
        assert_eq!(parse_tint("f4b860"), None);
        assert_eq!(parse_tint("#f4b6"), None);
        assert_eq!(parse_tint("#gggggg"), None);
    }
}
