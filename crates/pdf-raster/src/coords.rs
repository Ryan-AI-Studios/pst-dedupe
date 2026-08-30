//! Raster pixel (y-down) ↔ PDF user space (y-up) with CropBox origin + `/Rotate`.

use serde::{Deserialize, Serialize};

/// Axis-aligned box in PDF user space or pixel space.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct BoxF {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl BoxF {
    pub fn from_xywh(x: f64, y: f64, w: f64, h: f64) -> Self {
        Self { x, y, w, h }
    }

    pub fn from_corners(x0: f64, y0: f64, x1: f64, y1: f64) -> Self {
        let x = x0.min(x1);
        let y = y0.min(y1);
        Self {
            x,
            y,
            w: (x1 - x0).abs(),
            h: (y1 - y0).abs(),
        }
    }
}

/// Page boxes used to invert the raster transform.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PageBoxes {
    pub media: BoxF,
    pub crop: BoxF,
    pub rotate: i32,
}

/// Visible (rotated) width/height of `crop` in PDF points.
pub fn visual_size(crop: BoxF, rotate: i32) -> (f64, f64) {
    let cw = crop.w.max(0.0);
    let ch = crop.h.max(0.0);
    match normalize_rotate(rotate) {
        90 | 270 => (ch, cw),
        _ => (cw, ch),
    }
}

pub fn normalize_rotate(rotate: i32) -> i32 {
    let mut r = rotate % 360;
    if r < 0 {
        r += 360;
    }
    match r {
        90 | 180 | 270 => r,
        _ => 0,
    }
}

/// Map raster pixel box (y-down, origin top-left of the rendered image) to
/// PDF user space (y-up). Stored geom always uses this space for PDFs.
pub fn pixel_to_user_space(
    pixel: BoxF,
    raster_w: f64,
    raster_h: f64,
    crop: BoxF,
    rotate: i32,
) -> BoxF {
    let (vis_w, vis_h) = visual_size(crop, rotate);
    if raster_w <= 0.0 || raster_h <= 0.0 || vis_w <= 0.0 || vis_h <= 0.0 {
        return BoxF {
            x: crop.x,
            y: crop.y,
            w: 0.0,
            h: 0.0,
        };
    }
    let sx = vis_w / raster_w;
    let sy = vis_h / raster_h;
    let corners = [
        (pixel.x, pixel.y),
        (pixel.x + pixel.w, pixel.y),
        (pixel.x, pixel.y + pixel.h),
        (pixel.x + pixel.w, pixel.y + pixel.h),
    ];
    let mapped: Vec<(f64, f64)> = corners
        .into_iter()
        .map(|(px, py)| {
            let vis_x = px * sx;
            let vis_y = vis_h - py * sy;
            visual_to_user(vis_x, vis_y, crop, rotate)
        })
        .collect();
    let xs: Vec<f64> = mapped.iter().map(|p| p.0).collect();
    let ys: Vec<f64> = mapped.iter().map(|p| p.1).collect();
    let x0 = xs.iter().copied().fold(f64::INFINITY, f64::min);
    let x1 = xs.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let y0 = ys.iter().copied().fold(f64::INFINITY, f64::min);
    let y1 = ys.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    BoxF::from_corners(x0, y0, x1, y1)
}

/// Inverse of [`pixel_to_user_space`] for overlay paint.
pub fn user_space_to_pixel(
    user: BoxF,
    raster_w: f64,
    raster_h: f64,
    crop: BoxF,
    rotate: i32,
) -> BoxF {
    let (vis_w, vis_h) = visual_size(crop, rotate);
    if raster_w <= 0.0 || raster_h <= 0.0 || vis_w <= 0.0 || vis_h <= 0.0 {
        return BoxF {
            x: 0.0,
            y: 0.0,
            w: 0.0,
            h: 0.0,
        };
    }
    let sx = raster_w / vis_w;
    let sy = raster_h / vis_h;
    let corners = [
        (user.x, user.y),
        (user.x + user.w, user.y),
        (user.x, user.y + user.h),
        (user.x + user.w, user.y + user.h),
    ];
    let mapped: Vec<(f64, f64)> = corners
        .into_iter()
        .map(|(ux, uy)| {
            let (vis_x, vis_y) = user_to_visual(ux, uy, crop, rotate);
            let px = vis_x * sx;
            let py = (vis_h - vis_y) * sy;
            (px, py)
        })
        .collect();
    let xs: Vec<f64> = mapped.iter().map(|p| p.0).collect();
    let ys: Vec<f64> = mapped.iter().map(|p| p.1).collect();
    let x0 = xs.iter().copied().fold(f64::INFINITY, f64::min);
    let x1 = xs.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let y0 = ys.iter().copied().fold(f64::INFINITY, f64::min);
    let y1 = ys.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    BoxF::from_corners(x0, y0, x1, y1)
}

fn visual_to_user(vis_x: f64, vis_y: f64, crop: BoxF, rotate: i32) -> (f64, f64) {
    let cw = crop.w;
    let ch = crop.h;
    // zpdf `with_page_rotation` (clockwise): 90 maps (x,y) → (y, w-x).
    match normalize_rotate(rotate) {
        90 => (crop.x + cw - vis_y, crop.y + vis_x),
        180 => (crop.x + cw - vis_x, crop.y + ch - vis_y),
        270 => (crop.x + vis_y, crop.y + ch - vis_x),
        _ => (crop.x + vis_x, crop.y + vis_y),
    }
}

fn user_to_visual(ux: f64, uy: f64, crop: BoxF, rotate: i32) -> (f64, f64) {
    let rx = ux - crop.x;
    let ry = uy - crop.y;
    let cw = crop.w;
    let ch = crop.h;
    match normalize_rotate(rotate) {
        90 => (ry, cw - rx),
        180 => (cw - rx, ch - ry),
        270 => (ch - ry, rx),
        _ => (rx, ry),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotate0_roundtrip() {
        let crop = BoxF::from_xywh(0.0, 0.0, 612.0, 792.0);
        let px = BoxF::from_xywh(100.0, 50.0, 40.0, 20.0);
        let user = pixel_to_user_space(px, 612.0, 792.0, crop, 0);
        let back = user_space_to_pixel(user, 612.0, 792.0, crop, 0);
        assert!((back.x - px.x).abs() < 0.01);
        assert!((back.y - px.y).abs() < 0.01);
        assert!((back.w - px.w).abs() < 0.01);
        assert!((back.h - px.h).abs() < 0.01);
    }

    #[test]
    fn rotate90_roundtrip() {
        let crop = BoxF::from_xywh(0.0, 0.0, 612.0, 792.0);
        let (vis_w, vis_h) = visual_size(crop, 90);
        let px = BoxF::from_xywh(80.0, 40.0, 30.0, 16.0);
        let user = pixel_to_user_space(px, vis_w, vis_h, crop, 90);
        let back = user_space_to_pixel(user, vis_w, vis_h, crop, 90);
        assert!((back.x - px.x).abs() < 0.01);
        assert!((back.y - px.y).abs() < 0.01);
        assert!((back.w - px.w).abs() < 0.01);
        assert!((back.h - px.h).abs() < 0.01);
    }

    #[test]
    fn rotate90_matches_zpdf_clockwise_matrix() {
        // zpdf 90: (x,y) → (y, w-x). User BL (0,0) → visual y-up (0, w) = raster top-left.
        let crop = BoxF::from_xywh(0.0, 0.0, 612.0, 792.0);
        let (vis_w, vis_h) = visual_size(crop, 90);
        let user = BoxF::from_xywh(0.0, 0.0, 1.0, 1.0);
        let px = user_space_to_pixel(user, vis_w, vis_h, crop, 90);
        assert!(px.x < 2.0, "BL should sit at visual left, got x={}", px.x);
        assert!(
            px.y < 2.0,
            "BL should sit at raster top after y-flip, got y={}",
            px.y
        );
    }
}
