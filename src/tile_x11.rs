//! The desktop toggle icon, drawn DIRECTLY on X11 — no GUI toolkit, no
//! window manager cooperation required.
//!
//! The window is created with override-redirect set, exactly like conky
//! widgets and notification popups: Cinnamon (or any WM) never manages the
//! surface — no decorations, no shadow, no title bar, no taskbar entry, no
//! alt-tab entry. It is pixels floating on the desktop. This exists because
//! toolkit windows (winit/egui) kept getting treated as ordinary windows
//! no matter which hints we set.
//!
//! Everything pure (pixel rendering, scaling, packing, rounded-rect
//! geometry) is unit-tested; the X11 event loop itself needs a live display.

use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use x11rb::connection::Connection;
use x11rb::protocol::xproto::{
    AtomEnum, ChangeGCAux, ColormapAlloc, ConfigureWindowAux, ConnectionExt, CreateGCAux,
    CreateWindowAux, EventMask, ImageFormat, ImageOrder, PropMode, VisualClass, WindowClass,
};
use x11rb::protocol::Event;
use x11rb::rust_connection::RustConnection;

use crate::dictate::Signal;
use crate::studio::{
    decode_png_rgba, TileCtx, TileEvent, TilePointer, RECORDING, STATE, LOGO_PNG, DRAG_PX,
};

/// Side of the square icon surface, in pixels.
pub const TILE_PX: i16 = 104;
/// Gap from the bottom-right screen corner on first show.
const MARGIN: i16 = 24;
/// Menu item geometry (inside the surface, top area).
const MENU_H: i16 = 24;

// ---------------------------------------------------------------------
// Pure pixel helpers (unit-tested).
// ---------------------------------------------------------------------

/// Is point (px, py) inside the rounded rect (0,0,w,h) with corner radius r?
pub fn inside_rounded(px: i64, py: i64, w: i64, h: i64, r: i64) -> bool {
    inside_rounded_at(px, py, 0, 0, w, h, r)
}

/// Rounded-rect test against a rect anchored at (x0, y0).
fn inside_rounded_at(px: i64, py: i64, x0: i64, y0: i64, w: i64, h: i64, r: i64) -> bool {
    let (x, y) = (px - x0, py - y0);
    if x < 0 || y < 0 || x >= w || y >= h {
        return false;
    }
    let cx = x.clamp(r, w - 1 - r);
    let cy = y.clamp(r, h - 1 - r);
    let dx = x - cx;
    let dy = y - cy;
    dx * dx + dy * dy <= r * r
}

/// A rounded square with a colored ring: canvas of `size`×`size` RGBA
/// pixels, transparent outside, `ring` colored border of width `ring_w`,
/// `fill` colored inside.
pub fn rounded_square(size: u32, radius: u32, ring_w: u32, fill: [u8; 4], ring: [u8; 4]) -> Vec<u8> {
    let n = (size * size * 4) as usize;
    let mut out = vec![0u8; n];
    if size == 0 {
        return out;
    }
    let s = size as i64;
    let r = radius.min(size / 2 - 1).max(0) as i64;
    let rw = ring_w.min(size / 2) as i64;
    let r_in = (r - rw).max(0);
    for y in 0..size as i64 {
        for x in 0..size as i64 {
            if !inside_rounded(x, y, s, s, r) {
                continue; // stays transparent
            }
            // Ring = inside the outer shape but outside the shape inset by
            // ring_w on every side (constant border width on edges AND
            // corners).
            let color = if inside_rounded_at(x, y, rw, rw, s - 2 * rw, s - 2 * rw, r_in) {
                fill
            } else {
                ring
            };
            let i = ((y as usize * size as usize) + x as usize) * 4;
            out[i..i + 4].copy_from_slice(&color);
        }
    }
    out
}

/// Nearest-neighbor scale of an RGBA image.
pub fn scale_nearest(rgba: &[u8], w: u32, h: u32, tw: u32, th: u32) -> Vec<u8> {
    let mut out = vec![0u8; (tw * th * 4) as usize];
    if w == 0 || h == 0 || tw == 0 || th == 0 {
        return out;
    }
    for ty in 0..th {
        let sy = (ty * h / th) as usize;
        for tx in 0..tw {
            let sx = (tx * w / tw) as usize;
            let s = ((sy * w as usize) + sx) * 4;
            let d = ((ty as usize * tw as usize) + tx as usize) * 4;
            out[d..d + 4].copy_from_slice(&rgba[s..s + 4]);
        }
    }
    out
}

/// Alpha-blend `src` (w×h RGBA) over `dst` (canvas_w×canvas_h RGBA) at
/// (ox, oy). Clipped to the canvas.
pub fn composite_over(dst: &mut [u8], src: &[u8], ox: u32, oy: u32, w: u32, h: u32, canvas_w: u32) {
    for y in 0..h {
        let dy = oy + y;
        for x in 0..w {
            let dx = ox + x;
            let s = ((y * w + x) * 4) as usize;
            let sa = src[s + 3] as u32;
            if sa == 0 {
                continue;
            }
            let di = ((dy * canvas_w + dx) * 4) as usize;
            if di + 4 > dst.len() {
                continue;
            }
            if sa == 255 {
                dst[di..di + 4].copy_from_slice(&src[s..s + 4]);
                continue;
            }
            for c in 0..3 {
                let bg = dst[di + c] as u32;
                let fg = src[s + c] as u32;
                dst[di + c] = ((fg * sa + bg * (255 - sa)) / 255) as u8;
            }
            let ba = dst[di + 3] as u32;
            dst[di + 3] = (sa + ba * (255 - sa) / 255) as u8;
        }
    }
}

/// Pack RGBA pixels for a depth-32 ZPixmap put_image. The server's
/// image_byte_order decides the byte order inside each 32-bit pixel.
pub fn pack_for_put_image(rgba: &[u8], lsb_first: bool) -> Vec<u8> {
    rgba.chunks_exact(4)
        .flat_map(|p| {
            let [r, g, b, a] = [p[0], p[1], p[2], p[3]];
            if lsb_first {
                [b, g, r, a]
            } else {
                [a, r, g, b]
            }
        })
        .collect()
}

/// One rendered frame of the icon: state-colored rounded square + centered
/// logo + optional menu panel. Returns canvas_w, canvas_h, RGBA pixels.
pub fn render_frame(
    recording: bool,
    constant: bool,
    menu_open: bool,
    logo: Option<&(u32, u32, Vec<u8>)>,
) -> (u32, u32, Vec<u8>) {
    let size = TILE_PX as u32;
    let (fill, ring) = if recording {
        ([16u8, 46, 26, 242], [120u8, 255, 140, 255])
    } else if constant {
        ([14u8, 32, 46, 242], [120u8, 200, 255, 255])
    } else {
        ([12u8, 24, 16, 235], [34u8, 197, 94, 190])
    };
    let mut buf = rounded_square(size, 26, 2, fill, ring);
    if let Some((w, h, rgba)) = logo {
        let s = size - 28;
        let scaled = scale_nearest(rgba, *w, *h, s, s);
        composite_over(&mut buf, &scaled, 14, 10, s, s, size);
    }
    if menu_open {
        // Menu panel overlays the top of the icon: Studio / Hide rows.
        let panel = [10u8, 20, 14, 250];
        for y in 4u32..(4 + 2 * MENU_H as u32 + 8) {
            for x in 4..size - 4 {
                let i = ((y * size + x) * 4) as usize;
                if i + 4 <= buf.len() {
                    buf[i..i + 4].copy_from_slice(&panel);
                }
            }
        }
    }
    (size, size, buf)
}

// ---------------------------------------------------------------------
// The live X11 overlay.
// ---------------------------------------------------------------------

/// Run the toggle icon on X11. Blocks until hidden. `Err` when there is no
/// X11 display (e.g. Wayland-only) — the caller falls back to the egui tile.
pub fn run(c: TileCtx) -> Result<(), String> {
    let (conn, screen_num) = x11rb::connect(None).map_err(|e| format!("no X11 display: {e}"))?;
    let setup = conn.setup();
    let screen = setup
        .roots
        .get(screen_num)
        .ok_or_else(|| "bad X11 screen".to_string())?;
    let root = screen.root;
    let lsb = setup.image_byte_order == ImageOrder::LSB_FIRST;

    // Prefer a 32-bit ARGB visual so the surface can be transparent.
    let (depth, visual) = find_argb_visual(&setup, screen_num).unwrap_or_else(|| {
        (
            screen.root_depth,
            screen.root_visual,
        )
    });

    let win = conn.generate_id().map_err(|e| e.to_string())?;
    let cmap = conn.generate_id().map_err(|e| e.to_string())?;
    conn.create_colormap(ColormapAlloc::NONE, cmap, root, visual)
        .map_err(|e| e.to_string())?;

    let x = screen.width_in_pixels as i16 - TILE_PX - MARGIN;
    let y = screen.height_in_pixels as i16 - TILE_PX - MARGIN;

    let aux = CreateWindowAux::new()
        .colormap(cmap)
        .border_pixel(0)
        .override_redirect(1) // THE point: the WM never manages this window
        .event_mask(
            EventMask::EXPOSURE
                | EventMask::BUTTON_PRESS
                | EventMask::BUTTON_RELEASE
                | EventMask::POINTER_MOTION
                | EventMask::STRUCTURE_NOTIFY,
        );
    conn.create_window(
        depth,
        win,
        root,
        x,
        y,
        TILE_PX as u16,
        TILE_PX as u16,
        0,
        WindowClass::INPUT_OUTPUT,
        visual,
        &aux,
    )
    .map_err(|e| e.to_string())?;

    // Advertise dock type + above state; harmless for override-redirect,
    // correct if a compositor looks.
    for (prop, value) in [
        ("_NET_WM_WINDOW_TYPE", "_NET_WM_WINDOW_TYPE_DOCK"),
        ("_NET_WM_STATE", "_NET_WM_STATE_ABOVE"),
    ] {
        if let (Some(p), Some(v)) = (intern(&conn, prop), intern(&conn, value)) {
            let _ = conn.change_property(
                PropMode::REPLACE,
                win,
                p,
                AtomEnum::ATOM,
                32,
                1,
                &v.to_ne_bytes(),
            );
        }
    }
    let _ = conn.change_property(
        PropMode::REPLACE,
        win,
        intern(&conn, "_NET_WM_NAME").unwrap_or(AtomEnum::WM_NAME.into()),
        intern(&conn, "UTF8_STRING").unwrap_or(AtomEnum::STRING.into()),
        8,
        b"VoxBox".len() as u32,
        b"VoxBox",
    );

    let gc = conn.generate_id().map_err(|e| e.to_string())?;
    conn.create_gc(gc, win, &CreateGCAux::new().graphics_exposures(0))
        .map_err(|e| e.to_string())?;

    // Bitmap font for the tiny status word; silently absent = icon only.
    let font = conn.generate_id().map_err(|e| e.to_string())?;
    let font_ok = conn.open_font(font, b"fixed").is_ok();
    if font_ok {
        let _ = conn.change_gc(gc, &ChangeGCAux::new().font(font));
    }

    let logo = decode_png_rgba(LOGO_PNG);

    let _ = conn.map_window(win);
    let _ = conn.flush();

    // Gesture + interaction state.
    let mut pointer = TilePointer::new();
    let mut pressed = false;
    let mut last_xy = (0i16, 0i16);
    let mut pos = (x, y);
    let mut drag_dist = 0f32;
    let mut menu_open = false;
    let mut last_note = String::new();
    let mut last_sig: (bool, bool, bool, bool) = (false, false, false, true);
    let mut need_redraw = true;

    loop {
        while let Some(ev) = conn.poll_for_event().map_err(|e| e.to_string())? {
            match ev {
                Event::Expose(_) => need_redraw = true,
                Event::Error(_) => {}
                Event::ButtonPress(e) => {
                    if e.detail == 3 {
                        menu_open = !menu_open;
                        need_redraw = true;
                        continue;
                    }
                    if e.detail != 1 {
                        continue;
                    }
                    if menu_open {
                        // Clicks go to the menu, not to gestures.
                        let (mx, my) = (e.event_x, e.event_y);
                        let hit = menu_hit(mx, my);
                        match hit {
                            Some(0) => {
                                menu_open = false;
                                need_redraw = true;
                                crate::tray::open_studio();
                            }
                            Some(1) => {
                                // Hide: drop the connection, window dies.
                                return Ok(());
                            }
                            _ => {
                                menu_open = false;
                                need_redraw = true;
                            }
                        }
                        continue;
                    }
                    pressed = true;
                    last_xy = (e.event_x, e.event_y);
                    drag_dist = 0.0;
                    if let Some(ev) = pointer.step(true, false, Instant::now(), c.constant()) {
                        apply(&c, ev, &mut last_note);
                    }
                }
                Event::ButtonRelease(e) => {
                    if e.detail != 1 || !pressed {
                        continue;
                    }
                    pressed = false;
                    if let Some(ev) = pointer.step(false, true, Instant::now(), c.constant()) {
                        apply(&c, ev, &mut last_note);
                    }
                }
                Event::MotionNotify(e) => {
                    if !pressed {
                        continue;
                    }
                    let dx = e.event_x - last_xy.0;
                    let dy = e.event_y - last_xy.1;
                    last_xy = (e.event_x, e.event_y);
                    drag_dist += ((dx as f32) * (dx as f32) + (dy as f32) * (dy as f32)).sqrt();
                    if drag_dist > DRAG_PX {
                        // A drag is never a gesture.
                        pointer = TilePointer::new();
                        pos.0 += dx;
                        pos.1 += dy;
                        let _ = conn.configure_window(
                            win,
                            &ConfigureWindowAux::new()
                                .x(pos.0 as i32)
                                .y(pos.1 as i32),
                        );
                        let _ = conn.flush();
                    }
                }
                _ => {}
            }
        }

        // Drive the gesture machine while idle (holds, double-click window).
        let ev = pointer.step(pressed, false, Instant::now(), c.constant());
        if let Some(ev) = ev {
            apply(&c, ev, &mut last_note);
        }

        let recording = STATE.load(Ordering::Relaxed) == RECORDING;
        let constant = c.constant();
        let sig = (recording, constant, menu_open, pressed);
        if sig != last_sig || need_redraw {
            draw(&conn, win, gc, &logo, recording, constant, menu_open, lsb, font_ok);
            last_sig = sig;
            need_redraw = false;
        }

        std::thread::sleep(Duration::from_millis(50));
    }
}

fn apply(c: &TileCtx, ev: TileEvent, note: &mut String) {
    *note = match ev {
        TileEvent::Toggle => {
            let _ = c.trigger.as_ref().map(|t| t.send(Signal::Toggle));
            "one utterance…".to_string()
        }
        TileEvent::DoubleClick => {
            let next = !c.constant();
            if let Some(ctl) = &c.controls {
                ctl.constant.store(next, Ordering::Relaxed);
            }
            let _ = c.trigger.as_ref().map(|t| t.send(Signal::Constant(next)));
            if next {
                "constant mode ON".to_string()
            } else {
                "constant mode OFF".to_string()
            }
        }
        TileEvent::HoldStart => {
            let _ = c.trigger.as_ref().map(|t| t.send(Signal::HoldStart));
            "hold — talk, release to stop".to_string()
        }
        TileEvent::HoldEnd => {
            if let Some(ctl) = &c.controls {
                ctl.hold_release.store(true, Ordering::Relaxed);
            }
            "released — landing text…".to_string()
        }
    };
}

fn menu_hit(mx: i16, my: i16) -> Option<usize> {
    if mx < 4 || mx > TILE_PX - 4 {
        return None;
    }
    let top = 4 + MENU_H * 0;
    let bottom = 4 + MENU_H;
    if my >= top && my < top + MENU_H {
        return Some(0);
    }
    if my >= bottom && my < bottom + MENU_H + 4 {
        return Some(1);
    }
    None
}

fn draw(
    conn: &RustConnection,
    win: x11rb::protocol::xproto::Window,
    gc: x11rb::protocol::xproto::Gcontext,
    logo: &Option<(u32, u32, Vec<u8>)>,
    recording: bool,
    constant: bool,
    menu_open: bool,
    lsb: bool,
    font_ok: bool,
) {
    let (w, h, rgba) = render_frame(recording, constant, menu_open, logo.as_ref());
    let packed = pack_for_put_image(&rgba, lsb);
    if conn
        .put_image(
            ImageFormat::Z_PIXMAP,
            win,
            gc,
            w as u16,
            h as u16,
            0,
            0,
            0,
            32,
            &packed,
        )
        .is_err()
    {
        return;
    }
    // Tiny status word near the bottom, tinted by state.
    if font_ok {
        let word = if recording {
            "listening"
        } else if constant {
            "constant"
        } else {
            "tap"
        };
        let (r, g, b) = if recording {
            (120u32, 255, 140)
        } else if constant {
            (120, 200, 255)
        } else {
            (220, 255, 224)
        };
        let _ = conn.change_gc(gc, &ChangeGCAux::new().foreground((r << 16) | (g << 8) | b));
        let tx = TILE_PX / 2 - (word.len() as i16) * 3;
        let _ = conn.image_text8(win, gc, tx, TILE_PX - 8, word.as_bytes());
    }
    let _ = conn.flush();
}

fn intern(conn: &RustConnection, name: &str) -> Option<x11rb::protocol::xproto::Atom> {
    conn.intern_atom(false, name.as_bytes())
        .ok()?
        .reply()
        .ok()
        .map(|r| r.atom)
}

fn find_argb_visual(setup: &x11rb::protocol::xproto::Setup, screen_num: usize) -> Option<(u8, u32)> {
    let screen = setup.roots.get(screen_num)?;
    for d in &screen.allowed_depths {
        if d.depth == 32 {
            for v in &d.visuals {
                if v.class == VisualClass::TRUE_COLOR {
                    return Some((d.depth, v.visual_id));
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inside_rounded_matches_geometry() {
        // 10x10, radius 3: center inside, exact corner outside.
        assert!(inside_rounded(5, 5, 10, 10, 3));
        assert!(!inside_rounded(0, 0, 10, 10, 3));
        assert!(inside_rounded(0, 0, 10, 10, 0)); // r=0 -> plain rect
        assert!(!inside_rounded(-1, 5, 10, 10, 3));
        assert!(!inside_rounded(10, 5, 10, 10, 3));
    }

    #[test]
    fn rounded_square_transparent_outside_colored_inside() {
        let buf = rounded_square(40, 10, 2, [1, 2, 3, 200], [9, 9, 9, 255]);
        assert_eq!(buf.len(), 40 * 40 * 4);
        let px = |x: u32, y: u32| {
            let i = ((y * 40 + x) * 4) as usize;
            [buf[i], buf[i + 1], buf[i + 2], buf[i + 3]]
        };
        assert_eq!(px(0, 0), [0, 0, 0, 0], "corner must be transparent");
        assert_eq!(px(20, 20)[3], 200, "center must be fill");
        // A ring pixel: just inside the rounded edge, top-center.
        let edge = px(20, 1);
        assert_eq!(edge, [9, 9, 9, 255], "top-center pixel is ring");
    }

    #[test]
    fn scale_nearest_upscales_and_downscales() {
        // 1x1 red -> 2x2 red.
        let up = scale_nearest(&[255, 0, 0, 255], 1, 1, 2, 2);
        assert_eq!(up, vec![255, 0, 0, 255, 255, 0, 0, 255, 255, 0, 0, 255, 255, 0, 0, 255]);
        // 2x2 -> 1x1 picks top-left pixel.
        let two = vec![10, 0, 0, 255, 20, 0, 0, 255, 30, 0, 0, 255, 40, 0, 0, 255];
        let down = scale_nearest(&two, 2, 2, 1, 1);
        assert_eq!(down, vec![10, 0, 0, 255]);
    }

    #[test]
    fn composite_over_blends_alpha() {
        let mut dst = vec![0u8; 2 * 2 * 4];
        let src = vec![100u8, 0, 0, 128];
        composite_over(&mut dst, &src, 0, 0, 1, 1, 2);
        // 100 * 128/255 ≈ 50
        assert_eq!(dst[0], 50);
        assert_eq!(dst[3], 128);
        // Outside the 1x1 source stays untouched.
        assert_eq!(dst[4 + 3], 0);
        // Opaque source replaces.
        let src = vec![7u8, 8, 9, 255];
        composite_over(&mut dst, &src, 1, 1, 1, 1, 2);
        let i = (1 * 2 + 1) * 4;
        assert_eq!(&dst[i..i + 4], &[7, 8, 9, 255]);
    }

    #[test]
    fn pack_for_put_image_respects_byte_order() {
        let rgba = [1u8, 2, 3, 4]; // r=1 g=2 b=3 a=4
        assert_eq!(pack_for_put_image(&rgba, true), vec![3, 2, 1, 4]); // B,G,R,A
        assert_eq!(pack_for_put_image(&rgba, false), vec![4, 1, 2, 3]); // A,R,G,B
    }

    #[test]
    fn render_frame_changes_with_state() {
        let (w, h, idle) = render_frame(false, false, false, None);
        assert_eq!((w, h), (TILE_PX as u32, TILE_PX as u32));
        let (_, _, rec) = render_frame(true, false, false, None);
        assert_ne!(idle, rec, "recording frame must differ");
        let (_, _, con) = render_frame(false, true, false, None);
        assert_ne!(idle, con, "constant frame must differ");
        let (_, _, menu) = render_frame(false, false, true, None);
        assert_ne!(idle, menu, "open menu must paint the panel");
    }

    #[test]
    fn menu_hit_matches_the_two_rows() {
        assert_eq!(menu_hit(10, 10), Some(0), "Studio row");
        assert_eq!(menu_hit(10, 4 + MENU_H + 5), Some(1), "Hide row");
        assert_eq!(menu_hit(10, 90), None, "below the menu");
        assert_eq!(menu_hit(2, 10), None, "outside horizontally");
    }
}
