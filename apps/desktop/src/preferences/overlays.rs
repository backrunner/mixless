use super::*;
use crate::motion::{self, Presence};

#[derive(Clone)]
pub(super) enum ControllerPopup {
    Preset(PresetDialog),
    Import {
        report: Arc<mixless_controllers::ImportReport>,
        from_batch: bool,
    },
    Batch(Arc<mixless_controllers::ImportBatch>),
}

#[derive(Default)]
pub(super) struct Popups {
    reset: Presence<()>,
    controller: Presence<ControllerPopup>,
    select: Presence<Selector>,
}

impl Preferences {
    pub(super) fn render_popups(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> (Vec<gpui::AnyElement>, bool) {
        let now = cx.background_executor().now();
        let reduced = motion::reduce_motion();
        let mut popups = Vec::new();
        if let Some(frame) =
            self.popup_motion
                .reset
                .sample(self.confirm_reset.then_some(()), now, reduced)
            && let Some(popup) = frame.render("preferences-reset", window, |(), _| {
                Some(self.reset_dialog(cx))
            })
        {
            popups.push(popup);
        }
        let controller = if let Some(report) = &self.import_preview {
            Some(ControllerPopup::Import {
                report: report.clone(),
                from_batch: self.import_batch.is_some(),
            })
        } else if let Some(batch) = &self.import_batch {
            Some(ControllerPopup::Batch(batch.clone()))
        } else {
            self.preset_dialog.clone().map(ControllerPopup::Preset)
        };
        if let Some(frame) = self
            .popup_motion
            .controller
            .sample(controller, now, reduced)
            && let Some(popup) = frame.render("preferences-controller", window, |dialog, window| {
                Some(self.controller_dialog(
                    &dialog,
                    f32::from(window.viewport_size().height),
                    window,
                    cx,
                ))
            })
        {
            popups.push(popup);
        }
        if let Some(frame) = self.popup_motion.select.sample(self.selector, now, reduced)
            && let Some(popup) = frame.render("preferences-select", window, |selector, window| {
                Some(self.select_menu(selector, window, cx))
            })
        {
            popups.push(popup);
        }
        motion::stack(popups)
    }
}
