// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and Concat contributors

//! Platform composition and text-contrast diagnostics for motion comps.
//!
//! Safe-zone findings describe the node's authored anchor, not complete rendered
//! bounds: resolving arbitrary parent transforms, animated tracks, text metrics
//! and perspective belongs to a renderer-aware pass. Child nodes are omitted
//! rather than judged in the wrong coordinate space. Contrast is measured only
//! when foreground and its resolved background are known solid colours.

use crate::comp::{Comp, NodeKind, Paint, Rgba};

/// A target delivery format and its conservative title/action safe area.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SafeZoneProfile {
    /// Standard 16:9 landscape video.
    Landscape16x9,
    /// Vertical Shorts, Reels and TikTok; leaves room for platform UI.
    Vertical9x16,
    /// 4:5 feed creative.
    Feed4x5,
}

/// Infer a conservative safe-zone profile from a comp's frame dimensions.
///
/// Only the three explicitly supported delivery ratios are inferred. Custom
/// aspect ratios return `None` rather than being silently judged against
/// a profile that may not match the destination platform.
pub fn profile_for_dimensions(width: u32, height: u32) -> Option<SafeZoneProfile> {
    if width == 0 || height == 0 {
        return None;
    }
    let ratio = f64::from(width) / f64::from(height);
    [
        (16.0 / 9.0, SafeZoneProfile::Landscape16x9),
        (9.0 / 16.0, SafeZoneProfile::Vertical9x16),
        (4.0 / 5.0, SafeZoneProfile::Feed4x5),
    ]
    .into_iter()
    .find_map(|(expected, profile)| ((ratio - expected).abs() <= 0.02).then_some(profile))
}

impl SafeZoneProfile {
    /// Normalized (left, top, right, bottom) safe-area bounds.
    pub const fn bounds(self) -> (f64, f64, f64, f64) {
        match self {
            Self::Landscape16x9 => (0.05, 0.05, 0.95, 0.95),
            Self::Vertical9x16 => (0.08, 0.10, 0.92, 0.80),
            Self::Feed4x5 => (0.05, 0.05, 0.95, 0.95),
        }
    }

    /// Stable machine-readable profile name.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Landscape16x9 => "16:9",
            Self::Vertical9x16 => "9:16",
            Self::Feed4x5 => "4:5",
        }
    }
}

/// A platform/accessibility finding with a stable code and JSON pointer.
#[derive(Clone, Debug, PartialEq)]
pub struct AccessibilityFinding {
    /// Stable rule code (L600 safe-zone, L610 contrast).
    pub code: &'static str,
    /// JSON pointer to the affected node/property.
    pub path: String,
    /// Human-readable diagnostic.
    pub message: String,
    /// Suggested action; this module never silently changes a design.
    pub help: String,
}

/// Check authored text-node anchors against a delivery profile's safe area.
///
/// This is an anchor check, not a rendered-bounds check. It skips children
/// because their positions are parent-relative, and skips animated positions
/// because a single static point cannot represent their path.
pub fn audit_safe_zone(comp: &Comp, profile: SafeZoneProfile) -> Vec<AccessibilityFinding> {
    let (left, top, right, bottom) = profile.bounds();
    let mut findings = Vec::new();
    for (index, node) in comp.nodes.iter().enumerate() {
        let NodeKind::Text(_) = &node.kind else {
            continue;
        };
        if node.parent.is_some() || node.tracks.contains_key("position") {
            continue;
        }
        let position = node.transform.position.unwrap_or(crate::comp::Point {
            x: comp.width as f64 * 0.5,
            y: comp.height as f64 * 0.5,
        });
        let x = position.x / comp.width.max(1) as f64;
        let y = position.y / comp.height.max(1) as f64;
        if x < left || x > right || y < top || y > bottom {
            findings.push(AccessibilityFinding {
                code: "L600",
                path: format!("/nodes/{index}/transform/position"),
                message: format!(
                    "text node {:?} has its authored anchor at ({:.1}%, {:.1}%), outside the {} safe area",
                    node.id, x * 100.0, y * 100.0, profile.as_str()
                ),
                help: format!(
                    "move the text anchor inside {:.0}%–{:.0}% horizontally and {:.0}%–{:.0}% vertically; this check measures the anchor, not the full text bounds",
                    left * 100.0, right * 100.0, top * 100.0, bottom * 100.0
                ),
            });
        }
    }
    findings
}

/// WCAG relative luminance for an sRGB colour.
pub fn relative_luminance(color: Rgba) -> f64 {
    fn linear(channel: f64) -> f64 {
        let channel = channel.clamp(0.0, 1.0);
        if channel <= 0.04045 {
            channel / 12.92
        } else {
            ((channel + 0.055) / 1.055).powf(2.4)
        }
    }
    0.2126 * linear(color.r) + 0.7152 * linear(color.g) + 0.0722 * linear(color.b)
}

/// WCAG contrast ratio for foreground sRGB colour over an opaque background.
///
/// A translucent foreground is first composited over the background, so the
/// reported ratio reflects the colour a viewer actually sees. The background
/// is expected to be opaque; callers without a known opaque background should
/// skip the check rather than guess what sits behind it.
pub fn composite_over(foreground: Rgba, background: Rgba) -> Rgba {
    let alpha = foreground.a.clamp(0.0, 1.0);
    Rgba::new(
        foreground.r * alpha + background.r * (1.0 - alpha),
        foreground.g * alpha + background.g * (1.0 - alpha),
        foreground.b * alpha + background.b * (1.0 - alpha),
        1.0,
    )
}

pub fn contrast_ratio(foreground: Rgba, background: Rgba) -> f64 {
    let composite = composite_over(foreground, background);
    let a = relative_luminance(composite);
    let b = relative_luminance(background);
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}

/// Audit native text against a known opaque solid background.
///
/// Normal text requires 4.5:1; large text (at least 24px, or bold at least
/// 18.66px) requires 3:1. Gradient text and unknown/translucent backgrounds
/// are skipped because a single contrast ratio would be misleading.
pub fn audit_text_contrast(comp: &Comp) -> Vec<AccessibilityFinding> {
    let mut findings = Vec::new();
    for (index, node) in comp.nodes.iter().enumerate() {
        let NodeKind::Text(text) = &node.kind else {
            continue;
        };
        // A solid fill is a concrete foreground and can be measured;
        // gradients are skipped because one representative colour would lie.
        let foreground = match &text.fill {
            Some(Paint::Solid { color }) => *color,
            Some(Paint::Gradient { .. }) => continue,
            None => text.color,
        };
        let background = if text.background.a >= 0.999 {
            Some(text.background)
        } else if comp.background.a >= 0.999 {
            // A translucent text plate is drawn over the comp background;
            // compare against the resulting pixel, not the unblended plate
            // or the background underneath it.
            Some(composite_over(text.background, comp.background))
        } else {
            None
        };
        let Some(background) = background else {
            continue;
        };
        let ratio = contrast_ratio(foreground, background);
        let large = text.size >= 24.0 || (text.weight >= 700 && text.size >= 18.66);
        let required = if large { 3.0 } else { 4.5 };
        if ratio + 1e-9 < required {
            findings.push(AccessibilityFinding {
                code: "L610",
                path: format!("/nodes/{index}/color"),
                message: format!(
                    "text node {:?} has {:.2}:1 contrast; WCAG AA requires {:.1}:1 for {} text",
                    node.id, ratio, required, if large { "large" } else { "normal" }
                ),
                help: format!(
                    "choose a solid foreground/background pair with at least {:.1}:1 contrast; no automatic recolour is applied because it could change the design",
                    required
                ),
            });
        }
    }
    findings
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::comp::{Blend, Comp, Node, Point, TextNode, Transform};

    fn text_comp(color: Rgba, background: Rgba, size: f64, weight: u32) -> Comp {
        let node = Node {
            id: "headline".into(),
            parent: None,
            start: 0.0,
            duration: 0.0,
            transform: Transform {
                position: Some(Point::new(960.0, 540.0)),
                ..Transform::default()
            },
            opacity: 1.0,
            blend: Blend::default(),
            mask: None,
            matte: None,
            motion_blur: None,
            effects: Vec::new(),
            tracks: Default::default(),
            kind: NodeKind::Text(Box::new(TextNode {
                color,
                size,
                weight,
                ..TextNode::default()
            })),
        };
        Comp {
            format: crate::comp::FORMAT,
            id: "accessibility-test".into(),
            name: "Accessibility test".into(),
            width: 1920,
            height: 1080,
            fps: 30.0,
            duration: 2.0,
            background,
            assets: Default::default(),
            curves: Default::default(),
            nodes: vec![node],
            comps: Default::default(),
            slots: Vec::new(),
        }
    }

    #[test]
    fn dimensions_infer_only_supported_delivery_profiles() {
        assert_eq!(
            profile_for_dimensions(1920, 1080),
            Some(SafeZoneProfile::Landscape16x9)
        );
        assert_eq!(
            profile_for_dimensions(1080, 1920),
            Some(SafeZoneProfile::Vertical9x16)
        );
        assert_eq!(
            profile_for_dimensions(1080, 1350),
            Some(SafeZoneProfile::Feed4x5)
        );
        assert_eq!(profile_for_dimensions(1000, 1000), None);
        assert_eq!(profile_for_dimensions(0, 1080), None);
    }

    #[test]
    fn contrast_ratio_matches_wcag_reference_pairs() {
        assert!((contrast_ratio(Rgba::hex("#000000"), Rgba::hex("#ffffff")) - 21.0).abs() < 0.01);
        assert!((contrast_ratio(Rgba::hex("#777777"), Rgba::hex("#ffffff")) - 4.48).abs() < 0.03);
    }

    #[test]
    fn translucent_foreground_is_composited_before_contrast_is_measured() {
        let ratio = contrast_ratio(Rgba::new(0.0, 0.0, 0.0, 0.5), Rgba::hex("#ffffff"));
        assert!((ratio - 3.98).abs() < 0.02, "got {ratio:.3}");
    }

    #[test]
    fn normal_text_fails_below_aa_and_large_text_uses_three_to_one() {
        let normal = text_comp(Rgba::hex("#777777"), Rgba::hex("#ffffff"), 16.0, 400);
        assert_eq!(audit_text_contrast(&normal)[0].code, "L610");
        let large = text_comp(Rgba::hex("#777777"), Rgba::hex("#ffffff"), 24.0, 400);
        assert!(audit_text_contrast(&large).is_empty());
    }

    #[test]
    fn solid_text_fill_is_measured_but_gradient_fill_is_not_guessed() {
        let mut solid = text_comp(Rgba::hex("#ffffff"), Rgba::hex("#ffffff"), 16.0, 400);
        if let NodeKind::Text(text) = &mut solid.nodes[0].kind {
            text.fill = Some(Paint::Solid {
                color: Rgba::hex("#777777"),
            });
        }
        assert_eq!(audit_text_contrast(&solid)[0].code, "L610");

        if let NodeKind::Text(text) = &mut solid.nodes[0].kind {
            text.fill = Some(Paint::Gradient {
                stops: vec![(0.0, Rgba::hex("#777777")), (1.0, Rgba::hex("#ffffff"))],
                ramp: crate::comp::GradientKind::Linear,
                angle: 0.0,
                from: 0.0,
                to: 1.0,
            });
        }
        assert!(audit_text_contrast(&solid).is_empty());
    }

    #[test]
    fn translucent_text_plate_is_composited_over_opaque_comp_background() {
        let mut comp = text_comp(Rgba::hex("#000000"), Rgba::hex("#000000"), 16.0, 400);
        if let NodeKind::Text(text) = &mut comp.nodes[0].kind {
            text.background = Rgba::new(1.0, 1.0, 1.0, 0.5);
        }
        assert!(
            audit_text_contrast(&comp).is_empty(),
            "black text over a half-white plate on black has sufficient contrast"
        );
    }

    #[test]
    fn unknown_transparent_background_is_not_guessed() {
        let comp = text_comp(
            Rgba::hex("#777777"),
            Rgba::new(0.0, 0.0, 0.0, 0.0),
            16.0,
            400,
        );
        assert!(audit_text_contrast(&comp).is_empty());
    }

    #[test]
    fn vertical_safe_zone_flags_anchor_below_ui_safe_area() {
        let mut comp = text_comp(Rgba::hex("#ffffff"), Rgba::hex("#000000"), 32.0, 700);
        comp.width = 1080;
        comp.height = 1920;
        comp.nodes[0].transform.position = Some(Point::new(540.0, 1700.0));
        let findings = audit_safe_zone(&comp, SafeZoneProfile::Vertical9x16);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].code, "L600");
    }

    #[test]
    fn safe_zone_skips_parent_relative_and_animated_positions() {
        let mut comp = text_comp(Rgba::hex("#ffffff"), Rgba::hex("#000000"), 32.0, 700);
        comp.nodes[0].parent = Some("group".into());
        assert!(audit_safe_zone(&comp, SafeZoneProfile::Landscape16x9).is_empty());
        comp.nodes[0].parent = None;
        comp.nodes[0].tracks.insert(
            "position".into(),
            crate::track::Track::new(vec![
                crate::track::Key::new(0.0, crate::track::Value::Point { x: 0.0, y: 0.0 }),
                crate::track::Key::new(1.0, crate::track::Value::Point { x: 1.0, y: 1.0 }),
            ]),
        );
        assert!(audit_safe_zone(&comp, SafeZoneProfile::Landscape16x9).is_empty());
    }
}
