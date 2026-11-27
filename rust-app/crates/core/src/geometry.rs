//! Logical 96-DPI saved bounds and physical monitor work areas. No OS/UI calls.
//! Remember the monitor name to disambiguate absolute logical coordinates when
//! monitors use different scale factors; legacy bounds fall back to visibility.
use crate::settings::Bounds;

#[derive(Clone, Debug)]
pub struct Monitor {
    pub name: String,
    pub work_area: Bounds,
    pub scale: f64,
}
#[derive(Clone, Debug, PartialEq)]
pub struct Anchor {
    pub monitor: String,
    /// Logical offsets from the work-area origin, stable across DPI/layout changes.
    pub x: f64,
    pub y: f64,
}
#[derive(Clone, Debug, PartialEq)]
pub struct Placement {
    pub bounds: Bounds,
    pub anchor: Anchor,
}
#[derive(Clone, Debug)]
pub struct Restored {
    pub logical: Placement,
    pub physical: Bounds,
    pub scale: f64,
    pub work_area: Bounds,
}
fn valid(bounds: Bounds) -> bool {
    [bounds.x, bounds.y, bounds.width, bounds.height]
        .into_iter()
        .all(f64::is_finite)
        && bounds.width > 0.0
        && bounds.height > 0.0
}
impl Monitor {
    fn valid(&self) -> bool {
        valid(self.work_area) && self.scale.is_finite() && self.scale > 0.0
    }
    fn candidate(&self, saved: Bounds, minimum: (f64, f64), desktop: Bounds) -> Bounds {
        let limit = f64::from(i32::MAX - 1);
        Bounds {
            x: (saved.x * self.scale).clamp(-limit, limit),
            y: (saved.y * self.scale).clamp(-limit, limit),
            width: saved
                .width
                .max(minimum.0)
                .min((desktop.width / self.scale).max(minimum.0))
                * self.scale,
            height: saved
                .height
                .max(minimum.1)
                .min((desktop.height / self.scale).max(minimum.1))
                * self.scale,
        }
    }
}
fn overlap(a: Bounds, b: Bounds) -> f64 {
    let width = (a.x + a.width).min(b.x + b.width) - a.x.max(b.x);
    let height = (a.y + a.height).min(b.y + b.height) - a.y.max(b.y);
    width.max(0.0) * height.max(0.0)
}
fn distance(a: Bounds, b: Bounds) -> f64 {
    let dx = (b.x - a.x - a.width).max(a.x - b.x - b.width).max(0.0);
    let dy = (b.y - a.y - a.height).max(a.y - b.y - b.height).max(0.0);
    dx.hypot(dy)
}

/// Keep a usable part of the title/drag strip on a work area. Negative desktop
/// coordinates are valid. A disconnected monitor's placement moves to the
/// nearest current work area; no primary-screen-only positivity check is used.
pub fn restore(
    saved: Bounds,
    preferred: Option<&Anchor>,
    monitors: &[Monitor],
    minimum: (f64, f64),
) -> Option<Restored> {
    if !valid(saved)
        || !minimum.0.is_finite()
        || !minimum.1.is_finite()
        || minimum.0 <= 0.0
        || minimum.1 <= 0.0
    {
        return None;
    }
    let desktop = monitors.iter().filter(|monitor| monitor.valid()).fold(
        None::<Bounds>,
        |area, monitor| {
            Some(if let Some(area) = area {
                let work = monitor.work_area;
                let x = area.x.min(work.x);
                let y = area.y.min(work.y);
                Bounds {
                    x,
                    y,
                    width: (area.x + area.width).max(work.x + work.width) - x,
                    height: (area.y + area.height).max(work.y + work.height) - y,
                }
            } else {
                monitor.work_area
            })
        },
    )?;
    let preferred_monitor = monitors.iter().find(|monitor| {
        monitor.valid()
            && preferred.is_some_and(|anchor| {
                anchor.monitor == monitor.name && anchor.x.is_finite() && anchor.y.is_finite()
            })
    });
    let monitor = if let Some(monitor) = preferred_monitor {
        monitor
    } else {
        let mut best: Option<(&Monitor, f64, f64)> = None;
        for monitor in monitors.iter().filter(|monitor| monitor.valid()) {
            let candidate = monitor.candidate(saved, minimum, desktop);
            let visible = overlap(candidate, monitor.work_area);
            let gap = distance(candidate, monitor.work_area);
            if best.is_none_or(|(_, area, previous_gap)| {
                visible > area || visible == area && gap < previous_gap
            }) {
                best = Some((monitor, visible, gap));
            }
        }
        best?.0
    };
    let mut physical = monitor.candidate(saved, minimum, desktop);
    if let (Some(anchor), Some(_)) = (preferred, preferred_monitor) {
        let limit = f64::from(i32::MAX - 1);
        physical.x = (monitor.work_area.x + anchor.x * monitor.scale).clamp(-limit, limit);
        physical.y = (monitor.work_area.y + anchor.y * monitor.scale).clamp(-limit, limit);
    }
    if !valid(physical) {
        return None;
    }
    let work = monitor.work_area;
    let drag_width = (64.0 * monitor.scale).min(physical.width).min(work.width);
    let drag_height = (28.0 * monitor.scale).min(physical.height).min(work.height);
    let title = Bounds {
        height: drag_height,
        ..physical
    };
    let reachable = monitors
        .iter()
        .filter(|display| display.valid())
        .any(|display| {
            let work = display.work_area;
            let horizontal = (title.x + title.width).min(work.x + work.width) - title.x.max(work.x);
            let vertical = (title.y + title.height).min(work.y + work.height) - title.y.max(work.y);
            horizontal >= drag_width && vertical >= drag_height
        });
    // Full drag-strip height and at least 64 logical horizontal pixels must be
    // reachable, even when the body intentionally straddles another monitor.
    let horizontal = (title.x + title.width).min(work.x + work.width) - title.x.max(work.x);
    let vertical = (title.y + title.height).min(work.y + work.height) - title.y.max(work.y);
    if !reachable && horizontal < drag_width {
        physical.x = physical.x.clamp(
            work.x - physical.width + drag_width,
            work.x + work.width - drag_width,
        );
    }
    if !reachable && vertical < drag_height {
        physical.y = physical.y.clamp(work.y, work.y + work.height - drag_height);
    }
    let logical = capture(physical, monitor)?;
    Some(Restored {
        physical,
        logical,
        scale: monitor.scale,
        work_area: monitor.work_area,
    })
}

/// Snapshot an actual window only after excluding minimized/zero-sized states.
pub fn capture(physical: Bounds, monitor: &Monitor) -> Option<Placement> {
    if !valid(physical) || !monitor.valid() {
        return None;
    }
    let bounds = Bounds {
        x: physical.x / monitor.scale,
        y: physical.y / monitor.scale,
        width: physical.width / monitor.scale,
        height: physical.height / monitor.scale,
    };
    valid(bounds).then(|| Placement {
        bounds,
        anchor: Anchor {
            monitor: monitor.name.clone(),
            x: (physical.x - monitor.work_area.x) / monitor.scale,
            y: (physical.y - monitor.work_area.y) / monitor.scale,
        },
    })
}

#[derive(Clone, Copy, Debug, Default)]
pub struct AutoHeight {
    manual: bool,
}
impl AutoHeight {
    pub fn new(manual: bool) -> Self {
        Self { manual }
    }
    pub fn mark_manual(&mut self) {
        self.manual = true;
    }
    pub fn is_manual(self) -> bool {
        self.manual
    }
    /// Auto-fit until the user sets a window height.
    pub fn adjusted(self, current: f64, required: f64) -> f64 {
        if self.manual { current } else { required }
    }
}

/// A corner made from two cubics. Both outer handles lie on their
/// straight edges, so curvature is zero at the joins. The reflected
/// halves have matching tangent and curvature at the diagonal.
#[derive(Clone, Copy)]
pub struct SmoothCorner {
    pub start: [f32; 2],
    pub segments: [[[f32; 2]; 3]; 2],
}

#[derive(Clone, Copy)]
pub enum SmoothVertex {
    Sharp([f32; 2]),
    Curve(SmoothCorner),
}

pub fn smooth_corners([left, top, right, bottom]: [f32; 4], radius: f32) -> [SmoothCorner; 4] {
    // Extend the transition along the edges. At the diagonal this retains
    // the visible corner depth of the original nominal circular radius.
    let r = (radius * 1.65)
        .min((right - left) / 2.)
        .min((bottom - top) / 2.)
        .max(0.);
    let corner = |x, y, dx, dy| {
        let at = |along, across| [x + dx * along - dy * across, y + dy * along + dx * across];
        SmoothCorner {
            start: at(0., 0.),
            segments: [
                [at(0.275, 0.), at(0.6, 0.), at(0.8, 0.2)],
                [at(1., 0.4), at(1., 0.725), at(1., 1.)],
            ],
        }
    };
    [
        corner(right - r, top, r, 0.),
        corner(right, bottom - r, 0., r),
        corner(left + r, bottom, -r, 0.),
        corner(left, top + r, 0., -r),
    ]
}

pub fn smooth_corners_radii(
    [left, top, right, bottom]: [f32; 4],
    [tl, tr, br, bl]: [f32; 4],
) -> [SmoothVertex; 4] {
    let w = (right - left).max(0.);
    let h = (bottom - top).max(0.);

    let mut tl = (tl * 1.65).max(0.);
    let mut tr = (tr * 1.65).max(0.);
    let mut br = (br * 1.65).max(0.);
    let mut bl = (bl * 1.65).max(0.);

    let mut scale = 1.0f32;
    if tl + tr > w && tl + tr > 0. {
        scale = scale.min(w / (tl + tr));
    }
    if bl + br > w && bl + br > 0. {
        scale = scale.min(w / (bl + br));
    }
    if tl + bl > h && tl + bl > 0. {
        scale = scale.min(h / (tl + bl));
    }
    if tr + br > h && tr + br > 0. {
        scale = scale.min(h / (tr + br));
    }
    tl *= scale;
    tr *= scale;
    br *= scale;
    bl *= scale;

    let corner = |x, y, dx, dy, sharp_x, sharp_y, r: f32| {
        if r <= 0. {
            return SmoothVertex::Sharp([sharp_x, sharp_y]);
        }
        let at = |along, across| [x + dx * along - dy * across, y + dy * along + dx * across];
        SmoothVertex::Curve(SmoothCorner {
            start: at(0., 0.),
            segments: [
                [at(0.275, 0.), at(0.6, 0.), at(0.8, 0.2)],
                [at(1., 0.4), at(1., 0.725), at(1., 1.)],
            ],
        })
    };
    [
        corner(right - tr, top, tr, 0., right, top, tr),
        corner(right, bottom - br, 0., br, right, bottom, br),
        corner(left + bl, bottom, -bl, 0., left, bottom, bl),
        corner(left, top + tl, 0., -tl, left, top, tl),
    ]
}

#[cfg(test)]
mod tests {

    #[test]
    fn smooth_corner_curvature_matches_at_straight_edges_and_middle() {
        let subtract = |a: [f32; 2], b: [f32; 2]| [a[0] - b[0], a[1] - b[1]];
        let curvature = |tangent: [f32; 2], acceleration: [f32; 2]| {
            (tangent[0] * acceleration[1] - tangent[1] * acceleration[0])
                / tangent[0].hypot(tangent[1]).powi(3)
        };
        for corner in smooth_corners([0., 0., 100., 60.], 10.) {
            let [[b, c, midpoint], [d, e, end]] = corner.segments;
            let a = corner.start;
            let at_start = subtract(b, a);
            let at_left_join = subtract(midpoint, c);
            let at_right_join = subtract(d, midpoint);
            let at_end = subtract(end, e);
            assert!(curvature(at_start, subtract(subtract(c, b), at_start)).abs() < 1e-6);
            assert!(curvature(at_end, subtract(at_end, subtract(e, d))).abs() < 1e-6);
            let tangent_difference = subtract(at_left_join, at_right_join);
            assert!(tangent_difference[0].hypot(tangent_difference[1]) < 3e-5);
            let left = curvature(at_left_join, subtract(at_left_join, subtract(c, b)));
            let right = curvature(at_right_join, subtract(subtract(e, d), at_right_join));
            assert!((left - right).abs() < 1e-5);
        }
    }

    #[test]
    fn smooth_corners_radii_handles_mixed_sharp_and_narrow_corners() {
        let top_only = smooth_corners_radii([0., 0., 200., 56.], [10., 10., 0., 0.]);
        assert!(matches!(top_only[0], SmoothVertex::Curve(_)));
        assert!(matches!(top_only[1], SmoothVertex::Sharp([200., 56.])));
        assert!(matches!(top_only[2], SmoothVertex::Sharp([0., 56.])));
        assert!(matches!(top_only[3], SmoothVertex::Curve(_)));

        let narrow = smooth_corners_radii([0., 0., 12., 8.], [10., 10., 10., 10.]);
        for vertex in narrow {
            let SmoothVertex::Curve(corner) = vertex else {
                panic!("expected curved corner on non-zero narrow rect");
            };
            let [[_, _, _], [_, _, end]] = corner.segments;
            assert!(end[0] >= 0. && end[0] <= 12.);
            assert!(end[1] >= 0. && end[1] <= 8.);
        }
    }

    use super::*;
    fn bounds(x: f64, y: f64, width: f64, height: f64) -> Bounds {
        Bounds {
            x,
            y,
            width,
            height,
        }
    }
    fn monitor(name: &str, area: Bounds, scale: f64) -> Monitor {
        Monitor {
            name: name.into(),
            work_area: area,
            scale,
        }
    }
    #[test]
    fn negative_secondary_coordinates_round_trip_at_all_required_scales() {
        for scale in [1.0, 1.25, 1.5, 2.0] {
            let display = monitor("left", bounds(-3840.0, -200.0, 3840.0, 2160.0), scale);
            let saved = bounds(-1700.0, 100.0, 775.0, 360.0);
            let anchor = Anchor {
                monitor: "left".into(),
                x: saved.x - display.work_area.x / scale,
                y: saved.y - display.work_area.y / scale,
            };
            let result = restore(
                saved,
                Some(&anchor),
                std::slice::from_ref(&display),
                (750.0, 170.0),
            )
            .unwrap();
            assert_eq!(result.logical.bounds, saved);
            assert_eq!(result.physical.x, saved.x * scale);
            assert_eq!(capture(result.physical, &display).unwrap(), result.logical);
        }
    }
    #[test]
    fn preferred_monitor_disambiguates_mixed_dpi_and_survives_scale_change() {
        let monitors = [
            monitor("primary", bounds(0.0, 0.0, 1920.0, 1040.0), 1.0),
            monitor("right", bounds(1920.0, 0.0, 2560.0, 1400.0), 2.0),
        ];
        let saved = bounds(1000.0, 100.0, 775.0, 360.0);
        let anchor = Anchor {
            monitor: "right".into(),
            x: 40.0,
            y: 100.0,
        };
        let result = restore(saved, Some(&anchor), &monitors, (750.0, 170.0)).unwrap();
        assert_eq!(result.logical.bounds, saved);
        assert_eq!(result.physical.x, 2000.0);
        assert_eq!(result.physical.width, 1550.0);
        let changed = [monitor("right", bounds(1920.0, 0.0, 2560.0, 1400.0), 1.5)];
        let result = restore(saved, Some(&anchor), &changed, (750.0, 170.0)).unwrap();
        assert_eq!(result.physical.width, 1162.5);
        assert_eq!(result.physical.x, 1980.0);
        assert_eq!(result.logical.anchor, anchor);
        let relocated = [monitor(
            "right",
            bounds(-2560.0, -200.0, 2560.0, 1400.0),
            1.25,
        )];
        let result = restore(saved, Some(&anchor), &relocated, (750.0, 170.0)).unwrap();
        assert_eq!(result.physical.x, -2510.0);
        assert_eq!(result.physical.y, -75.0);
        assert_eq!(result.logical.anchor, anchor);
    }
    #[test]
    fn disconnected_monitor_moves_to_nearest_available_work_area_and_keeps_drag_strip() {
        let monitors = [
            monitor("primary", bounds(0.0, 40.0, 1920.0, 1040.0), 1.0),
            monitor("left", bounds(-1920.0, 0.0, 1920.0, 1080.0), 1.0),
        ];
        let result = restore(
            bounds(-9000.0, -9000.0, 775.0, 170.0),
            Some(&Anchor {
                monitor: "gone".into(),
                x: 20.0,
                y: 20.0,
            }),
            &monitors,
            (750.0, 170.0),
        )
        .unwrap();
        assert_eq!(result.logical.anchor.monitor, "left");
        assert!(result.physical.x + result.physical.width >= -1920.0 + 64.0);
        assert_eq!(result.physical.y, 0.0);
    }
    #[test]
    fn partial_position_is_preserved_but_inaccessible_title_is_repaired() {
        let displays = [monitor("primary", bounds(0.0, 40.0, 1920.0, 1000.0), 1.0)];
        let saved = bounds(-200.0, 80.0, 775.0, 360.0);
        assert_eq!(
            restore(saved, None, &displays, (750.0, 170.0))
                .unwrap()
                .logical
                .bounds,
            saved
        );
        let result = restore(
            bounds(100.0, -100.0, 775.0, 360.0),
            None,
            &displays,
            (750.0, 170.0),
        )
        .unwrap();
        assert_eq!(result.physical.y, 40.0);
    }
    #[test]
    fn spanning_horizontal_monitors_keeps_the_users_saved_width() {
        let monitors = [
            monitor("primary", bounds(0.0, 0.0, 1920.0, 1040.0), 1.0),
            monitor("right", bounds(1920.0, 0.0, 1920.0, 1040.0), 1.0),
        ];
        let saved = bounds(100.0, 100.0, 2700.0, 600.0);
        let anchor = Anchor {
            monitor: "primary".into(),
            x: 100.0,
            y: 100.0,
        };
        assert_eq!(
            restore(saved, Some(&anchor), &monitors, (750.0, 170.0))
                .unwrap()
                .logical
                .bounds,
            saved
        );
    }
    #[test]
    fn spanning_vertical_monitors_keeps_a_title_reachable_on_the_other_monitor() {
        let monitors = [
            monitor("upper", bounds(0.0, 0.0, 1920.0, 1000.0), 1.0),
            monitor("lower", bounds(0.0, 1000.0, 1920.0, 1000.0), 1.0),
        ];
        let saved = bounds(100.0, 900.0, 775.0, 1000.0);
        assert_eq!(
            restore(saved, None, &monitors, (750.0, 170.0))
                .unwrap()
                .logical
                .bounds,
            saved
        );
    }
    #[test]
    fn migration_defaults_honor_minimum_and_bad_or_extreme_bounds_do_not_wrap() {
        let displays = [monitor("primary", bounds(0.0, 0.0, 1920.0, 1040.0), 1.0)];
        let default = bounds(572.5, 643.0, 775.0, 167.0);
        assert_eq!(
            restore(default, None, &displays, (750.0, 170.0))
                .unwrap()
                .logical
                .bounds
                .height,
            170.0
        );
        let giant = restore(
            bounds(f64::MAX, -f64::MAX, f64::MAX, f64::MAX),
            None,
            &displays,
            (750.0, 170.0),
        )
        .unwrap();
        assert_eq!(giant.physical.width, 1920.0);
        assert_eq!(giant.physical.height, 1040.0);
        assert!(giant.physical.x <= 1920.0 - 64.0);
        assert_eq!(giant.physical.y, 0.0);
        assert!(
            restore(
                bounds(f64::NAN, 0.0, 775.0, 170.0),
                None,
                &displays,
                (750.0, 170.0)
            )
            .is_none()
        );
        assert!(restore(default, None, &[], (750.0, 170.0)).is_none());
    }
    #[test]
    fn manual_height_can_grow_for_cards_without_later_automatic_shrink() {
        let mut policy = AutoHeight::default();
        assert_eq!(policy.adjusted(360.0, 170.0), 170.0);
        policy.mark_manual();
        assert_eq!(policy.adjusted(500.0, 220.0), 500.0);
        assert_eq!(policy.adjusted(500.0, 660.0), 500.0);
        assert_eq!(policy.adjusted(660.0, 170.0), 660.0);
        assert!(policy.is_manual());
        assert!(AutoHeight::new(true).is_manual());
    }
}
