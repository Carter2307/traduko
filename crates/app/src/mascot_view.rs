//! Coco on the desktop: a small transparent window that only shows the
//! mascot. A press and a drag moves it; a press without a drag is a click.

use std::time::{Duration, Instant};

use coco_blob::{CANVAS, Frame, Mascot, Mood};
use gpui::{
    App, Bounds, Context, DispatchPhase, Entity, EventEmitter, MouseButton, MouseDownEvent, MouseExitEvent,
    MouseMoveEvent, MouseUpEvent, Path, PathBuilder, Pixels, Point, Rgba, Task, Window,
    WindowBackgroundAppearance, WindowBounds, WindowHandle, WindowKind, WindowOptions, canvas, div,
    point, prelude::*, px, size,
};

use objc2::rc::Retained;
use objc2_app_kit::NSWindow;

use crate::native::{self, Floating};

/// Pointer travel that turns a press into a drag, in points.
const DRAG_THRESHOLD: f32 = 4.0;
/// How close the pointer must come for Coco to look at it, in mascot units
/// (about 230 points for the medium size).
const NOTICE: f32 = 6.0;
/// How far from the centre a press still lands on the body, in mascot units.
const BODY: f32 = 1.3;
/// How often a still Coco checks where the pointer is.
const WATCH: Duration = Duration::from_millis(110);

pub enum MascotEvent {
    Clicked,
    /// Dropped at a new place after a drag.
    Moved,
}

struct Press {
    origin: Point<Pixels>,
    /// Pointer minus window origin on screen, set once the press became a drag.
    grab: Option<(f64, f64)>,
    last_pointer: (f64, f64),
    last_at: Instant,
}

pub struct MascotView {
    mascot: Mascot,
    /// Side of the square window, in points.
    side: f32,
    last_tick: Instant,
    /// Frames drawn since the last report, with `COCO_FPS=1`.
    frames: Option<(u32, Instant)>,
    press: Option<Press>,
    /// Drag speed in mascot units per second, smoothed.
    velocity: [f32; 2],
    wake: Option<Task<()>>,
    /// The native window, to know where Coco is on screen.
    window: Option<Retained<NSWindow>>,
    /// The pointer as last given to the mascot.
    seen: Option<[f32; 2]>,
    _watch: Task<()>,
}

impl EventEmitter<MascotEvent> for MascotView {}

impl MascotView {
    fn new(mood: Mood, side: f32, cx: &mut Context<Self>) -> Self {
        let mut mascot = Mascot::new(mood);
        mascot.enter();

        // A still Coco draws nothing, so nothing tells it that the pointer
        // came close: this looks a few times per second and wakes it up.
        let watch = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(WATCH).await;
                let alive = this.update(cx, |this, cx| {
                    if this.wake.is_some() && moved(this.pointer_near(), this.seen) {
                        cx.notify();
                    }
                });
                if alive.is_err() {
                    break;
                }
            }
        });

        Self {
            mascot,
            side,
            last_tick: Instant::now(),
            frames: std::env::var_os("COCO_FPS").map(|_| (0, Instant::now())),
            press: None,
            velocity: [0.0; 2],
            wake: None,
            window: None,
            seen: None,
            _watch: watch,
        }
    }

    /// Points per unit of the mascot's own coordinates.
    fn scale(&self) -> f32 {
        self.side / (2.0 * CANVAS)
    }

    /// Changes Coco's size. The window grows or shrinks around its centre
    /// and stays on screen.
    pub fn set_side(&mut self, side: f32, cx: &mut Context<Self>) {
        if side == self.side {
            return;
        }
        self.side = side;
        self.mascot.nudge();
        cx.notify();
        let Some(win) = self.window.clone() else { return };
        // AppKit reports the new size back to gpui at once, which must not
        // happen inside a gpui update: a task runs after it.
        cx.spawn(async move |this, cx| {
            let (centre_x, centre_y) = native::frame(&win).center();
            let side = f64::from(side);
            let wanted = native::Rect { x: centre_x - side / 2.0, y: centre_y - side / 2.0, w: side, h: side };
            let placed = native::visible_area_at(wanted.center()).map_or(wanted, |area| wanted.kept_inside(&area));
            native::set_frame(&win, placed);
            // Its corner moved: the place is saved and the panel follows.
            this.update(cx, |_, cx| cx.emit(MascotEvent::Moved)).ok();
        })
        .detach();
    }

    /// The pointer relative to Coco's centre, in mascot units, when it is
    /// close enough to be noticed.
    fn pointer_near(&self) -> Option<[f32; 2]> {
        let frame = native::frame(self.window.as_ref()?);
        let (x, y) = native::pointer();
        let (cx, cy) = frame.center();
        let scale = self.scale();
        // Screen y grows upwards, the mascot's y grows downwards.
        let local = [(x - cx) as f32 / scale, (cy - y) as f32 / scale];
        (local[0].hypot(local[1]) < NOTICE).then_some(local)
    }

    pub fn set_mood(&mut self, mood: Mood, cx: &mut Context<Self>) {
        self.mascot.set_mood(mood);
        cx.notify();
    }

    /// Turns the eyes towards a direction (x right, y down), or back to the front.
    pub fn look_towards(&mut self, direction: Option<[f32; 2]>, cx: &mut Context<Self>) {
        self.mascot.set_attention(direction);
        cx.notify();
    }

    /// A slow breath while the panel is open and Coco has company. Off
    /// otherwise, so that an idle Coco costs no drawing at all.
    pub fn set_breathing(&mut self, on: bool, cx: &mut Context<Self>) {
        self.mascot.breathe = on;
        cx.notify();
    }

    pub fn nudge(&mut self, cx: &mut Context<Self>) {
        self.mascot.nudge();
        cx.notify();
    }

    /// Advances the motion to now and books the next frame: the next display
    /// refresh while something moves, a timer for the next blink otherwise.
    fn tick(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let now = Instant::now();
        let dt = now.duration_since(self.last_tick).as_secs_f32();
        self.last_tick = now;
        if let Some((count, since)) = &mut self.frames {
            *count += 1;
            if now.duration_since(*since) >= Duration::from_secs(1) {
                println!("mascot frames {count} mood {:?} resting {}", self.mascot.mood(), self.mascot.is_resting());
                (*count, *since) = (0, now);
            }
        }

        match &self.press {
            Some(press) if press.grab.is_some() => {
                // The pointer stopped but the button is still down: the body catches up.
                if now.duration_since(press.last_at) > Duration::from_millis(40) {
                    self.velocity = self.velocity.map(|v| v * (-dt / 0.06).exp());
                }
                self.mascot.drag(self.velocity);
            }
            Some(_) => {}
            None => {
                self.seen = self.pointer_near();
                self.mascot.set_pointer(self.seen);
            }
        }

        self.mascot.step(dt);

        if self.mascot.is_resting() {
            let delay = Duration::from_secs_f32(self.mascot.next_wake().max(0.02));
            self.wake = Some(cx.spawn(async move |this, cx| {
                cx.background_executor().timer(delay).await;
                this.update(cx, |_, cx| cx.notify()).ok();
            }));
        } else if self.only_breathing() {
            // A slow breath reads the same at 30 frames a second.
            self.wake = Some(cx.spawn(async move |this, cx| {
                cx.background_executor().timer(Duration::from_millis(33)).await;
                this.update(cx, |_, cx| cx.notify()).ok();
            }));
        } else {
            self.wake = None;
            window.request_animation_frame();
        }
    }

    /// True when the breath is the only thing that moves.
    fn only_breathing(&mut self) -> bool {
        if !self.mascot.breathe {
            return false;
        }
        self.mascot.breathe = false;
        let otherwise_resting = self.mascot.is_resting();
        self.mascot.breathe = true;
        otherwise_resting
    }

    fn on_down(&mut self, event: &MouseDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        // The window is a square; Coco is the body in its middle.
        let (half, scale) = (self.side / 2.0, self.scale());
        let from_centre = ((event.position.x.as_f32() - half) / scale).hypot((event.position.y.as_f32() - half) / scale);
        if from_centre > BODY {
            return;
        }
        self.press = Some(Press {
            origin: event.position,
            grab: None,
            last_pointer: native::pointer(),
            last_at: Instant::now(),
        });
        self.mascot.press();
        cx.notify();
    }

    fn on_move(&mut self, event: &MouseMoveEvent, window: &mut Window, cx: &mut Context<Self>) {
        let scale = self.scale();
        let Some(press) = self.press.as_mut() else {
            // The next frame reads the pointer itself.
            cx.notify();
            return;
        };

        if event.pressed_button != Some(MouseButton::Left) {
            // The button went up somewhere we did not see.
            self.end_press(false, cx);
            return;
        }

        let Some(win) = native::ns_window(window) else { return };
        let pointer = native::pointer();
        match press.grab {
            None => {
                let dx = (event.position.x - press.origin.x).as_f32();
                let dy = (event.position.y - press.origin.y).as_f32();
                if dx * dx + dy * dy > DRAG_THRESHOLD * DRAG_THRESHOLD {
                    let frame = native::frame(&win);
                    press.grab = Some((pointer.0 - frame.x, pointer.1 - frame.y));
                    press.last_pointer = pointer;
                    press.last_at = Instant::now();
                    self.mascot.set_pointer(None);
                }
            }
            Some(grab) => {
                // The speed is sampled over at least a 120 Hz frame and smoothed
                // over time, so every mouse gives the same pose for the same pull.
                let now = Instant::now();
                let dt = now.duration_since(press.last_at).as_secs_f32();
                if dt >= 1.0 / 120.0 {
                    // Screen y grows upwards, the mascot's y grows downwards.
                    let v = [
                        (pointer.0 - press.last_pointer.0) as f32 / dt / scale,
                        -(pointer.1 - press.last_pointer.1) as f32 / dt / scale,
                    ];
                    let keep = (-dt / 0.04).exp();
                    self.velocity = [self.velocity[0] * keep + v[0] * (1.0 - keep), self.velocity[1] * keep + v[1] * (1.0 - keep)];
                    press.last_pointer = pointer;
                    press.last_at = now;
                }
                // AppKit reports the move back to gpui at once, which must not
                // happen while gpui is inside this window's own handler.
                cx.spawn(async move |_, _| native::set_origin(&win, pointer.0 - grab.0, pointer.1 - grab.1)).detach();
            }
        }
        cx.notify();
    }

    fn on_up(&mut self, event: &MouseUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        // The second half of a double-click must not close what the first opened.
        self.end_press(event.click_count <= 1, cx);
    }

    fn on_exit(&mut self, _: &MouseExitEvent, _: &mut Window, cx: &mut Context<Self>) {
        cx.notify();
    }

    fn end_press(&mut self, released_here: bool, cx: &mut Context<Self>) {
        let Some(press) = self.press.take() else { return };
        self.velocity = [0.0; 2];
        if press.grab.is_some() {
            self.mascot.drop_it();
            // After the last move of the window, which is itself queued.
            cx.spawn(async move |this, cx| {
                this.update(cx, |_, cx| cx.emit(MascotEvent::Moved)).ok();
            })
            .detach();
        } else {
            self.mascot.release();
            if released_here {
                cx.emit(MascotEvent::Clicked);
            }
        }
        cx.notify();
    }
}

impl Render for MascotView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.tick(window, cx);
        let frame = self.mascot.frame();
        let entity = cx.entity();

        div().size_full().child(
            canvas(
                |_, _, _| (),
                move |bounds, _, window, _| {
                    paint(&frame, bounds, window);

                    // The window is the mascot, so every mouse event of this
                    // window is ours. AppKit keeps sending the drag and the
                    // release to the view that got the press, even once the
                    // pointer is outside the window.
                    let this = entity.clone();
                    window.on_mouse_event(move |event: &MouseDownEvent, phase, window, cx| {
                        if phase == DispatchPhase::Bubble && event.button == MouseButton::Left {
                            this.update(cx, |this, cx| this.on_down(event, window, cx));
                        }
                    });
                    let this = entity.clone();
                    window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, cx| {
                        if phase == DispatchPhase::Bubble {
                            this.update(cx, |this, cx| this.on_move(event, window, cx));
                        }
                    });
                    let this = entity.clone();
                    window.on_mouse_event(move |event: &MouseUpEvent, phase, window, cx| {
                        if phase == DispatchPhase::Bubble && event.button == MouseButton::Left {
                            this.update(cx, |this, cx| this.on_up(event, window, cx));
                        }
                    });
                    let this = entity.clone();
                    window.on_mouse_event(move |event: &MouseExitEvent, phase, window, cx| {
                        if phase == DispatchPhase::Bubble {
                            this.update(cx, |this, cx| this.on_exit(event, window, cx));
                        }
                    });
                },
            )
            .size_full(),
        )
    }
}

/// True when the pointer came, went, or moved enough to be worth a redraw.
fn moved(now: Option<[f32; 2]>, before: Option<[f32; 2]>) -> bool {
    match (now, before) {
        (Some(a), Some(b)) => (a[0] - b[0]).hypot(a[1] - b[1]) > 0.04,
        (None, None) => false,
        _ => true,
    }
}

/// Draws the body, then the eyes over it, centred in `bounds`.
pub fn paint(frame: &Frame, bounds: Bounds<Pixels>, window: &mut Window) {
    let [r, g, b] = frame.color;
    if let Some(body) = outline(&frame.body, bounds) {
        window.paint_path(body, Rgba { r, g, b, a: 1.0 });
    }
    for eye in &frame.eyes {
        if let Some(eye) = outline(eye, bounds) {
            window.paint_path(eye, Rgba { r: 1.0, g: 1.0, b: 1.0, a: 1.0 });
        }
    }
}

/// A closed, smooth curve through the middle of each pair of neighbours,
/// with the points themselves as control points.
fn outline(points: &[[f32; 2]], bounds: Bounds<Pixels>) -> Option<Path<Pixels>> {
    if points.len() < 3 {
        return None;
    }
    let centre = bounds.center();
    let scale = bounds.size.width.as_f32() / (2.0 * CANVAS);
    let at = |p: [f32; 2]| point(centre.x + px(p[0] * scale), centre.y + px(p[1] * scale));
    let mid = |a: [f32; 2], b: [f32; 2]| at([(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0]);

    let n = points.len();
    let mut path = PathBuilder::fill();
    path.move_to(mid(points[n - 1], points[0]));
    for i in 0..n {
        path.curve_to(mid(points[i], points[(i + 1) % n]), at(points[i]));
    }
    path.close();
    path.build().ok()
}

/// Opens the mascot window, a square of `side` points, with its bottom-left
/// corner at `origin` (screen coordinates), or in the bottom-right corner of
/// the display the pointer is on.
pub fn open(
    cx: &mut App,
    origin: Option<(f64, f64)>,
    mood: Mood,
    side: f32,
) -> anyhow::Result<(WindowHandle<MascotView>, Entity<MascotView>)> {
    let points = side;
    let side = px(points);
    let mut view = None;
    let handle = cx.open_window(
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds::new(point(px(200.), px(200.)), size(side, side)))),
            titlebar: None,
            window_background: WindowBackgroundAppearance::Transparent,
            kind: WindowKind::PopUp,
            // Never takes the keyboard, and stays hidden until it is placed.
            focus: false,
            show: false,
            is_resizable: false,
            is_minimizable: false,
            // We move the window ourselves, from anywhere on the mascot.
            app_owns_titlebar_drag: true,
            // The app is never the active one: without this the animation
            // would be held to a few frames per second.
            inactive_frame_interval: None,
            ..Default::default()
        },
        |window, cx| {
            native::float(window, Floating::Mascot);
            let mascot = cx.new(|cx| MascotView::new(mood, points, cx));
            view = Some(mascot.clone());
            mascot
        },
    )?;
    let view = view.ok_or_else(|| anyhow::anyhow!("the mascot view was not built"))?;

    let win = handle.update(cx, |_, window, _| native::ns_window(window))?;
    if let Some(win) = win {
        let side = f64::from(points);
        let wanted = match origin {
            Some((x, y)) => native::Rect { x, y, w: side, h: side },
            None => {
                let area = native::visible_area_at(native::pointer());
                let (right, bottom) = area.map_or((1200.0, 0.0), |a| (a.x + a.w, a.y));
                native::Rect { x: right - side - 28.0, y: bottom + 28.0, w: side, h: side }
            }
        };
        let placed = native::visible_area_at(wanted.center()).map_or(wanted, |area| wanted.kept_inside(&area));
        native::set_origin(&win, placed.x, placed.y);
        native::show(&win);
        view.update(cx, |mascot, _| mascot.window = Some(win));
        // The move happened while gpui was busy with this very call, so it
        // missed AppKit's report: on a display with another scale the mascot
        // would be drawn at the wrong size until its first drag.
        handle.update(cx, |_, window, cx| window.bounds_changed(cx))?;
    }
    Ok((handle, view))
}
