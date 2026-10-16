//! Initial window sizes.
//!
//! The launcher (`tauri.conf.json`, 1288 px) and the main Web UI window
//! (`WEB_UI_HEIGHT`) open about 40 % taller than they used to (920 and 840
//! px), so more of the launcher and the Web Chat shows without scrolling.
//! That is taller than many laptop screens, so each window is then fitted to
//! the work area (the screen without the menu bar, Dock or taskbar) of the
//! display it opens on.
use tauri::{PhysicalSize, Runtime, WebviewWindow};

/// Height of the main Web UI window, in logical pixels (was 840).
pub const WEB_UI_HEIGHT: f64 = 1176.0;

/// Shrink `window` to fit the work area of its display and centre it there.
/// A window that already fits is left alone. Failures are logged and leave
/// the window as it is, because a too-tall window is still usable.
pub fn fit_to_screen<R: Runtime>(window: &WebviewWindow<R>) {
    if let Err(error) = try_fit(window) {
        log::warn!(
            "could not fit window {} to the screen: {error}",
            window.label()
        );
    }
}

fn try_fit<R: Runtime>(window: &WebviewWindow<R>) -> tauri::Result<()> {
    let monitor = match window.current_monitor()? {
        Some(monitor) => Some(monitor),
        None => window.primary_monitor()?,
    };
    let Some(monitor) = monitor else {
        log::info!(
            "no display found for window {}; keeping its size",
            window.label()
        );
        return Ok(());
    };
    let area = monitor.work_area().size;
    let outer = window.outer_size()?;
    let inner = window.inner_size()?;
    let Some(fitted) = fitted_inner_size(inner, outer, area) else {
        return Ok(());
    };
    log::info!(
        "fitting window {} to the screen: {}x{} -> {}x{} physical pixels",
        window.label(),
        inner.width,
        inner.height,
        fitted.width,
        fitted.height
    );
    window.set_size(fitted)?;
    window.center()
}

/// The inner size that keeps the whole window (frame included) inside
/// `area`, or `None` when it already fits.
fn fitted_inner_size(
    inner: PhysicalSize<u32>,
    outer: PhysicalSize<u32>,
    area: PhysicalSize<u32>,
) -> Option<PhysicalSize<u32>> {
    let frame_width = outer.width.saturating_sub(inner.width);
    let frame_height = outer.height.saturating_sub(inner.height);
    let width = inner.width.min(area.width.saturating_sub(frame_width));
    let height = inner.height.min(area.height.saturating_sub(frame_height));
    (width != inner.width || height != inner.height).then(|| PhysicalSize::new(width, height))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_window_taller_than_the_screen_keeps_its_frame_inside_the_work_area() {
        let size = PhysicalSize::new;
        // 1288 logical px at 2x on a 14-inch MacBook work area, with a 28 px title bar.
        let fitted = fitted_inner_size(size(2360, 2576), size(2360, 2632), size(3024, 1890));
        assert_eq!(fitted, Some(size(2360, 1834)));
        // A window that fits is left alone.
        assert_eq!(
            fitted_inner_size(size(2360, 1600), size(2360, 1656), size(3024, 1890)),
            None
        );
        // Too wide as well: both sides shrink.
        assert_eq!(
            fitted_inner_size(size(2000, 1200), size(2000, 1200), size(1600, 900)),
            Some(size(1600, 900))
        );
    }
}
