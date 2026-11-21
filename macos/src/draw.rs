//! Paints the widget with AppKit, using the same layout and colors as Windows
//! (`usage_core::layout`). Coordinates are points in a flipped view (y grows downwards),
//! so the numbers match the Windows code one-to-one.

use objc2::msg_send;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_app_kit::{NSBezierPath, NSColor, NSFont, NSFontAttributeName, NSForegroundColorAttributeName};
use objc2_foundation::{NSDictionary, NSPoint, NSRect, NSSize, NSString};
use usage_core::layout::{self, level_color, Rgb, CLOSE, COL_BAR, COL_LABEL, COL_PCT, COL_REMAIN, NOTE_H, PAD, ROW_H, W};
use usage_core::view::{Status, ViewModel};

/// Point size of all widget text (SF, the system font).
pub const FONT_SIZE: f64 = 12.0;

pub fn regular_font() -> Retained<NSFont> {
    NSFont::systemFontOfSize(FONT_SIZE)
}

pub fn bold_font() -> Retained<NSFont> {
    NSFont::boldSystemFontOfSize(FONT_SIZE)
}

fn color(c: Rgb) -> Retained<NSColor> {
    NSColor::colorWithSRGBRed_green_blue_alpha(c.0 as f64 / 255.0, c.1 as f64 / 255.0, c.2 as f64 / 255.0, 1.0)
}

fn rect(left: f64, top: f64, width: f64, height: f64) -> NSRect {
    NSRect::new(NSPoint::new(left, top), NSSize::new(width, height))
}

/// Text attributes: font and color.
fn attrs(font: &NSFont, color: &NSColor) -> Retained<NSDictionary<NSString, AnyObject>> {
    let keys: [&NSString; 2] = unsafe { [NSFontAttributeName, NSForegroundColorAttributeName] };
    let font: &AnyObject = font;
    let color: &AnyObject = color;
    NSDictionary::from_slices(&keys, &[font, color])
}

/// Size of `s` drawn on one line in `font`.
pub fn text_size(s: &str, font: &NSFont) -> NSSize {
    let ns = NSString::from_str(s);
    let a = attrs(font, &NSColor::blackColor());
    unsafe { msg_send![&*ns, sizeWithAttributes: &*a] }
}

#[derive(Clone, Copy)]
enum Align {
    Left,
    Right,
    Center,
}

/// Draw one line of text inside the horizontal span `left..right`, vertically centered
/// in `top..top + height`.
fn draw_line(s: &str, font: &NSFont, c: Rgb, (left, right): (f64, f64), top: f64, height: f64, align: Align) {
    let ns = NSString::from_str(s);
    let a = attrs(font, &color(c));
    let size: NSSize = unsafe { msg_send![&*ns, sizeWithAttributes: &*a] };
    let x = match align {
        Align::Left => left,
        Align::Right => right - size.width,
        Align::Center => left + (right - left - size.width) / 2.0,
    };
    let y = top + (height - size.height) / 2.0;
    let _: () = unsafe { msg_send![&*ns, drawAtPoint: NSPoint::new(x, y), withAttributes: &*a] };
}

fn fill(r: NSRect, c: Rgb) {
    color(c).setFill();
    NSBezierPath::fillRect(r);
}

fn span(c: (i32, i32)) -> (f64, f64) {
    (c.0 as f64, c.1 as f64)
}

/// The close (✕) box, in view coordinates. Matches the Windows hit area.
pub fn close_rect() -> NSRect {
    rect((W - CLOSE - 4) as f64, 4.0, CLOSE as f64, CLOSE as f64)
}

/// Paint the whole widget for `vm` into the current graphics context.
pub fn paint(vm: &ViewModel, height: f64) {
    let (regular, bold) = (regular_font(), bold_font());
    let w = W as f64;

    // Rounded dark background; the window itself is transparent, so the corners are too.
    color(layout::BG).setFill();
    NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(rect(0.0, 0.0, w, height), 6.0, 6.0).fill();

    let c = close_rect();
    draw_line("✕", &regular, layout::DIM, (c.origin.x, c.origin.x + c.size.width), c.origin.y, c.size.height, Align::Center);

    match vm {
        ViewModel::Message(m) => {
            let (left, right) = (PAD as f64, (W - CLOSE - 8) as f64);
            if text_size(m, &regular).width <= right - left {
                draw_line(m, &regular, layout::TEXT, (left, right), 0.0, height, Align::Left);
            } else {
                // Too long for one line: let AppKit wrap it inside the text area.
                let ns = NSString::from_str(m);
                let a = attrs(&regular, &color(layout::TEXT));
                let r = rect(left, PAD as f64, right - left, height - 2.0 * PAD as f64);
                let _: () = unsafe { msg_send![&*ns, drawInRect: r, withAttributes: &*a] };
            }
        }
        ViewModel::Rows { rows, status, note } => {
            let stale = *status == Status::Stale;
            let text = if stale { layout::DIM } else { layout::TEXT };
            for (i, row) in rows.iter().enumerate() {
                let top = (PAD + ROW_H * i as i32) as f64;
                let h = ROW_H as f64;
                draw_line(&row.label, &bold, text, span(COL_LABEL), top, h, Align::Left);

                // Bar: a 6 pt track, filled up to the usage fraction.
                let (bl, br) = span(COL_BAR);
                let mid = top + h / 2.0;
                fill(rect(bl, mid - 3.0, br - bl, 6.0), layout::TRACK);
                let filled = ((br - bl) * row.fill).round();
                if filled > 0.0 {
                    fill(rect(bl, mid - 3.0, filled, 6.0), level_color(row.level, stale));
                }

                draw_line(&row.pct_text, &regular, text, span(COL_PCT), top, h, Align::Right);
                draw_line(&row.remaining, &regular, layout::DIM, span(COL_REMAIN), top, h, Align::Left);
            }
            if let Some(n) = note {
                let top = (PAD + ROW_H * rows.len() as i32) as f64;
                draw_line(n, &regular, layout::DIM, (PAD as f64, w - PAD as f64), top, NOTE_H as f64, Align::Left);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn columns_fit_longest_strings_mac() {
        let (regular, bold) = (regular_font(), bold_font());
        let width = |c: (i32, i32)| (c.1 - c.0) as f64;
        for s in ["Session", "Weekly", "Opus wk", "Sonnet wk"] {
            assert!(text_size(s, &bold).width <= width(COL_LABEL), "label {s:?} clipped");
        }
        for s in ["resetting…", "23h 59m", "6d 23h", "4d 23h", "59m"] {
            assert!(text_size(s, &regular).width <= width(COL_REMAIN), "countdown {s:?} clipped");
        }
        for s in ["100%", "130%"] {
            assert!(text_size(s, &regular).width <= width(COL_PCT), "percent {s:?} clipped");
        }
    }
}
