//! M0 spike: prove the render budget (10k quads @ 60 fps), infinite pan, zoom-at-cursor,
//! and measure frame cadence on real GPU compositing.
//!
//! Throwaway by design — the durable viewport lands with the scene model at M3/M4.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use gpui::{
    App, Application, Bounds, Context, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, Point,
    Render, ScrollDelta, ScrollWheelEvent, SharedString, Window, WindowBounds, WindowOptions,
    canvas, div, prelude::*, px, quad, rgb, size,
};
/// World-space rectangle `[x, y, w, h]` in model units.
type WorldRect = [f64; 4];

const QUAD_COUNT: usize = 10_000;
const MIN_ZOOM: f64 = 0.05;
const MAX_ZOOM: f64 = 20.0;
const FRAME_WINDOW: usize = 120;

pub fn run_spike() {
    Application::new().run(|cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(1280.), px(800.)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                focus: true,
                ..Default::default()
            },
            |window, cx| cx.new(|cx| SpikeView::new(window, cx)),
        )
        .unwrap();
        cx.on_window_closed(|cx| cx.quit()).detach();
        cx.activate(true);
        println!("spike: window opened");
    });
}

struct FrameMeter {
    last_frame: Option<Instant>,
    /// Rolling sum of recent frame deltas, microseconds.
    window_us: u64,
    window_len: usize,
    fps_x10: u32,
}

impl FrameMeter {
    fn new() -> Self {
        Self {
            last_frame: None,
            window_us: 0,
            window_len: 0,
            fps_x10: 0,
        }
    }

    fn tick(&mut self) {
        let now = Instant::now();
        if let Some(last) = self.last_frame {
            // Clamp stalls so one hiccup can't dominate the average.
            let dt = now.duration_since(last).as_micros().min(2_000_000) as u64;
            self.window_us += dt;
            self.window_len += 1;
            if self.window_len == FRAME_WINDOW {
                let avg_us = self.window_us / FRAME_WINDOW as u64;
                self.fps_x10 = ((1_000_000.0 / avg_us as f64) * 10.0).round() as u32;
                println!(
                    "spike: avg frame {avg_us} us ({:.1} fps) over last {FRAME_WINDOW} frames",
                    self.fps_x10 as f64 / 10.0
                );
                self.window_us = 0;
                self.window_len = 0;
            }
        }
        self.last_frame = Some(now);
    }
}

/// Deterministic LCG so every run lays out identical quads.
fn gen_quads(n: usize) -> Vec<WorldRect> {
    let mut state: u64 = 0x2545_F491_4F6C_DD1D;
    let mut next = move || {
        state = state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (state >> 33) as f64 / u32::MAX as f64
    };
    (0..n)
        .map(|_| {
            let col = next() * 100.0;
            let row = next() * 90.0;
            [
                60.0 + col * 150.0 + next() * 40.0,
                60.0 + row * 110.0 + next() * 30.0,
                24.0 + next() * 56.0,
                20.0 + next() * 44.0,
            ]
        })
        .collect()
}

fn quad_color(i: usize) -> gpui::Rgba {
    const PALETTE: [u32; 8] = [
        0x5B8DEF, 0x51C28A, 0xE0A63C, 0xD96570, 0x9A6DD7, 0x4FC1C9, 0xC97BA8, 0x8C9440,
    ];
    rgb(PALETTE[i % PALETTE.len()])
}

fn pt(x: f64, y: f64) -> Point<Pixels> {
    Point {
        x: px(x as f32),
        y: px(y as f32),
    }
}

pub struct SpikeView {
    pan: Point<Pixels>,
    zoom: f64,
    dragging: bool,
    last_mouse: Point<Pixels>,
    meter_started: bool,
    meter: FrameMeter,
    quads: Arc<Vec<WorldRect>>,
    visible_count: Arc<AtomicUsize>,
}

impl SpikeView {
    fn new(_window: &mut Window, _cx: &mut Context<Self>) -> Self {
        Self {
            pan: pt(80.0, 80.0),
            zoom: 1.0,
            dragging: false,
            last_mouse: pt(0.0, 0.0),
            meter_started: false,
            meter: FrameMeter::new(),
            quads: Arc::new(gen_quads(QUAD_COUNT)),
            visible_count: Arc::new(AtomicUsize::new(0)),
        }
    }

    /// Continuous-frame loop: re-arms itself each frame so the meter samples real
    /// cadence even while idle. Dropped once interaction-driven invalidation lands.
    fn pump_frames(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.meter.tick();
        cx.notify();
        cx.on_next_frame(window, SpikeView::pump_frames);
    }
}

impl Render for SpikeView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.meter_started {
            self.meter_started = true;
            cx.on_next_frame(window, SpikeView::pump_frames);
        }

        let pan = self.pan;
        let zoom = self.zoom;
        let quads = Arc::clone(&self.quads);
        let visible_count = Arc::clone(&self.visible_count);

        let status = format!(
            "zoom {:.2}×   pan ({:.0}, {:.0})   quads {}/{}   {:.1} fps   drag=pan  wheel=zoom",
            zoom,
            pan.x,
            pan.y,
            visible_count.load(Ordering::Relaxed),
            QUAD_COUNT,
            self.meter.fps_x10 as f32 / 10.0,
        );

        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(0x14161A))
            .text_color(rgb(0xC9CDD4))
            .child(
                div()
                    .flex_grow()
                    .child(
                        canvas(
                            move |_, _, _| {},
                            move |bounds, (), window, _| {
                                paint_scene(bounds, pan, zoom, &quads, &visible_count, window);
                            },
                        )
                        .size_full(),
                    )
                    .on_mouse_down(
                        gpui::MouseButton::Left,
                        cx.listener(|this, ev: &MouseDownEvent, _, _| {
                            this.dragging = true;
                            this.last_mouse = ev.position;
                        }),
                    )
                    .on_mouse_move(cx.listener(|this, ev: &MouseMoveEvent, _, cx| {
                        if !this.dragging {
                            return;
                        }
                        let delta = ev.position - this.last_mouse;
                        this.pan.x += delta.x;
                        this.pan.y += delta.y;
                        this.last_mouse = ev.position;
                        cx.notify();
                    }))
                    .on_mouse_up(
                        gpui::MouseButton::Left,
                        cx.listener(|this, _: &MouseUpEvent, _, _| {
                            this.dragging = false;
                        }),
                    )
                    .on_scroll_wheel(cx.listener(|this, ev: &ScrollWheelEvent, _, cx| {
                        let dy = match ev.delta {
                            ScrollDelta::Pixels(d) => f64::from(d.y),
                            ScrollDelta::Lines(d) => f64::from(d.y) * 24.0,
                        };
                        let cursor = ev.position;
                        // The world point under the cursor stays fixed while zooming.
                        let world_x = (f64::from(cursor.x) - f64::from(this.pan.x)) / this.zoom;
                        let world_y = (f64::from(cursor.y) - f64::from(this.pan.y)) / this.zoom;
                        this.zoom = (this.zoom * (-dy * 0.0018).exp()).clamp(MIN_ZOOM, MAX_ZOOM);
                        this.pan.x = px((f64::from(cursor.x) - world_x * this.zoom) as f32);
                        this.pan.y = px((f64::from(cursor.y) - world_y * this.zoom) as f32);
                        cx.notify();
                    })),
            )
            .child(
                div()
                    .h(px(28.))
                    .px(px(12.))
                    .flex()
                    .items_center()
                    .bg(rgb(0x1B1E24))
                    .text_size(px(12.))
                    .font_family("monospace")
                    .child(SharedString::from(status)),
            )
    }
}

fn paint_scene(
    bounds: Bounds<Pixels>,
    pan: Point<Pixels>,
    zoom: f64,
    quads: &[WorldRect],
    visible_count: &AtomicUsize,
    window: &mut Window,
) {
    window.paint_quad(quad(
        bounds,
        px(0.),
        rgb(0x14161A),
        px(0.),
        gpui::transparent_black(),
        Default::default(),
    ));

    draw_grid(
        bounds,
        pan,
        zoom,
        gpui::hsla(0.58, 0.05, 0.55, 0.18),
        window,
    );

    let origin = bounds.origin;
    let w = f64::from(bounds.size.width);
    let h = f64::from(bounds.size.height);

    let mut visible = 0usize;
    for (i, r) in quads.iter().enumerate() {
        let sx = r[0] * zoom + f64::from(pan.x);
        let sy = r[1] * zoom + f64::from(pan.y);
        let sw = r[2] * zoom;
        let sh = r[3] * zoom;
        if sx + sw < 0.0 || sy + sh < 0.0 || sx > w || sy > h {
            continue;
        }
        visible += 1;
        window.paint_quad(quad(
            Bounds {
                origin: pt(f64::from(origin.x) + sx, f64::from(origin.y) + sy),
                size: size(px(sw as f32), px(sh as f32)),
            },
            px(0.),
            quad_color(i),
            px(0.),
            gpui::transparent_black(),
            Default::default(),
        ));
    }
    visible_count.store(visible, Ordering::Relaxed);
}

fn draw_grid(
    bounds: Bounds<Pixels>,
    pan: Point<Pixels>,
    zoom: f64,
    color: gpui::Hsla,
    window: &mut Window,
) {
    let w = bounds.size.width;
    let h = bounds.size.height;
    // Adaptive spacing keeps the on-screen cell between ~40px and ~160px.
    let mut step = 100.0 * zoom;
    while step < 40.0 {
        step *= 2.0;
    }
    while step > 160.0 {
        step /= 2.0;
    }

    let mut x = (-f64::from(pan.x) / step).floor() * step;
    while x <= f64::from(w) {
        window.paint_quad(quad(
            Bounds {
                origin: pt(x + f64::from(pan.x), 0.0),
                size: size(px(1.), h),
            },
            px(0.),
            color,
            px(0.),
            gpui::transparent_black(),
            Default::default(),
        ));
        x += step;
    }

    let mut y = (-f64::from(pan.y) / step).floor() * step;
    while y <= f64::from(h) {
        window.paint_quad(quad(
            Bounds {
                origin: pt(0.0, y + f64::from(pan.y)),
                size: size(w, px(1.)),
            },
            px(0.),
            color,
            px(0.),
            gpui::transparent_black(),
            Default::default(),
        ));
        y += step;
    }
}
