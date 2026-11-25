use gpui::{
    AnyElement, Bounds, Hsla, IntoElement, ParentElement, PathBuilder, Pixels, Styled, Window,
    canvas, point, px,
};

fn path(bounds: Bounds<Pixels>, radius: f32, mut path: PathBuilder) -> PathBuilder {
    let xy = |[x, y]: [f32; 2]| point(px(x), px(y));
    let rect = [
        bounds.origin.x,
        bounds.origin.y,
        bounds.right(),
        bounds.bottom(),
    ]
    .map(f32::from);
    let corners = captions_core::geometry::smooth_corners(rect, radius);
    path.move_to(xy(corners[0].start));
    for corner in corners {
        path.line_to(xy(corner.start));
        for [first, second, end] in corner.segments {
            path.cubic_bezier_to(xy(end), xy(first), xy(second));
        }
    }
    path.close();
    path
}

pub fn paint_fill(bounds: Bounds<Pixels>, color: Hsla, radius: f32, window: &mut Window) {
    if color.a > 0.
        && bounds.size.width > px(0.)
        && bounds.size.height > px(0.)
        && let Ok(shape) = path(bounds, radius, PathBuilder::fill()).build()
    {
        window.paint_path(shape, color);
    }
}

pub fn paint_outline(bounds: Bounds<Pixels>, color: Hsla, radius: f32, window: &mut Window) {
    if color.a > 0.
        && bounds.size.width > px(1.)
        && bounds.size.height > px(1.)
        && let Ok(shape) = path(
            bounds.dilate(px(-0.5)),
            radius - 0.5,
            PathBuilder::stroke(px(1.)),
        )
        .build()
    {
        window.paint_path(shape, color);
    }
}

pub fn layer(fill: Option<Hsla>, stroke: Option<Hsla>, radius: f32) -> AnyElement {
    canvas(
        |_, _, _| (),
        move |bounds, _, window, _| {
            if let Some(fill) = fill {
                paint_fill(bounds, fill, radius, window);
            }
            if let Some(stroke) = stroke {
                paint_outline(bounds, stroke, radius, window);
            }
        },
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full()
    .into_any_element()
}

pub trait ContinuousCorners: Styled + ParentElement + Sized {
    fn contour(self, fill: Option<Hsla>, stroke: Option<Hsla>, radius: f32) -> Self {
        self.child(layer(fill, stroke, radius))
    }
}
impl<T: Styled + ParentElement> ContinuousCorners for T {}
