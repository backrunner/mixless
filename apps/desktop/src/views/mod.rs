pub mod about;
mod audio;
mod automix;
pub mod deck;
mod ellipsized_text;
pub mod library;
pub mod mixer;
pub mod shortcuts;
pub mod topbar;

mod url_input;

use gpui::prelude::*;
use gpui::{IntoElement, Styled, Window, px};

use crate::theme;
pub(crate) use ellipsized_text::ellipsized_text;

/// Shared hover tooltip that wraps long names and paths.
pub(crate) struct TextTip(pub(crate) String);
impl gpui::Render for TextTip {
    fn render(&mut self, _: &mut Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
        gpui::div()
            .max_w(px(400.))
            .px_2()
            .py_1()
            .rounded(px(4.))
            .bg(theme::PANEL_RAISED)
            .text_size(px(11.))
            .text_color(theme::TEXT)
            .whitespace_normal()
            .child(self.0.clone())
    }
}
