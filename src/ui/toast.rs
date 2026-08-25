use eframe::egui::{self, Align2, Frame, RichText, Rounding, Stroke, Vec2};
use std::time::{Duration, Instant};

use crate::ui::theme::{colors, metrics};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToastKind {
    Success,
    Info,
    Warning,
    Error,
}

#[derive(Debug, Clone)]
pub struct Toast {
    pub message: String,
    pub kind: ToastKind,
    pub created_at: Instant,
    pub duration: Duration,
}

impl Toast {
    pub fn success(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            kind: ToastKind::Success,
            created_at: Instant::now(),
            duration: Duration::from_secs(3),
        }
    }

    pub fn info(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            kind: ToastKind::Info,
            created_at: Instant::now(),
            duration: Duration::from_secs(3),
        }
    }

    pub fn warning(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            kind: ToastKind::Warning,
            created_at: Instant::now(),
            duration: Duration::from_secs(4),
        }
    }

    pub fn error(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            kind: ToastKind::Error,
            created_at: Instant::now(),
            duration: Duration::from_secs(5),
        }
    }

    fn is_expired(&self) -> bool {
        self.created_at.elapsed() >= self.duration
    }
}

pub fn render_toasts(ctx: &egui::Context, toasts: &mut Vec<Toast>) {
    toasts.retain(|t| !t.is_expired());

    if toasts.is_empty() {
        return;
    }

    ctx.request_repaint_after(Duration::from_millis(100));

    egui::Area::new(egui::Id::new("toast_overlay"))
        .anchor(Align2::RIGHT_BOTTOM, Vec2::new(-20.0, -20.0))
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            ui.set_max_width(340.0);
            ui.vertical(|ui| {
                for toast in toasts.iter().rev() {
                    let (bg, border, fg, icon) = match toast.kind {
                        ToastKind::Success => (colors::OK_BG, colors::OK, colors::OK, "\u{2713}"),
                        ToastKind::Info => (
                            colors::ACCENT_BG,
                            colors::ACCENT,
                            colors::ACCENT,
                            "\u{2139}",
                        ),
                        ToastKind::Warning => {
                            (colors::WARN_BG, colors::WARN, colors::WARN, "\u{26a0}")
                        }
                        ToastKind::Error => (colors::ERR_BG, colors::ERR, colors::ERR, "\u{2717}"),
                    };

                    let elapsed = toast.created_at.elapsed().as_secs_f32();
                    let total = toast.duration.as_secs_f32();
                    let alpha = if elapsed > total - 0.5 {
                        ((total - elapsed) / 0.5).clamp(0.0, 1.0)
                    } else {
                        1.0
                    };

                    let bg_a = bg.linear_multiply(alpha);
                    let border_a = border.linear_multiply(alpha);
                    let fg_a = fg.linear_multiply(alpha);
                    let text_a = colors::TEXT.linear_multiply(alpha);

                    Frame::none()
                        .fill(bg_a)
                        .stroke(Stroke::new(1.0_f32, border_a))
                        .rounding(Rounding::same(metrics::ROUNDING_MD))
                        .inner_margin(Vec2::new(12.0, 8.0))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new(icon)
                                        .size(metrics::FS_BODY)
                                        .strong()
                                        .color(fg_a),
                                );
                                ui.add_space(6.0);
                                ui.label(
                                    RichText::new(&toast.message)
                                        .size(metrics::FS_LABEL)
                                        .strong()
                                        .color(text_a),
                                );
                            });
                        });
                    ui.add_space(6.0);
                }
            });
        });
}
