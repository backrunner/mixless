//! Root popup composition, including retained content during dismissal.
use crate::{
    motion::{self, Presence},
    state::UiState,
};
use gpui::{AnyElement, Context, Pixels, Point, Window};
use mixless_protocol::DeckId;

#[derive(Default)]
pub(crate) struct Popups {
    audio: Presence<()>,
    track: Presence<crate::views::library::TrackMenu>,
    playlist: Presence<crate::views::library::PlaylistMenu>,
    library: Presence<Point<Pixels>>,
    deck: Presence<crate::views::deck::DeckMenu>,
    brand: Presence<()>,
    remove_playlist: Presence<(i64, String)>,
    import: Presence<()>,
    export: Presence<()>,
    shortcuts: Presence<()>,
    fx: Presence<(DeckId, usize)>,
    master: Presence<()>,
}

impl UiState {
    pub(super) fn render_popups(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> (Vec<AnyElement>, bool) {
        let now = cx.background_executor().now();
        let reduced = motion::reduce_motion();
        let mut popups = Vec::new();
        macro_rules! popup {
            ($field:ident, $target:expr, $render:expr) => {
                if let Some(frame) = self.popup_motion.$field.sample($target, now, reduced)
                    && let Some(popup) =
                        frame.render(concat!("popup-", stringify!($field)), window, $render)
                {
                    popups.push(popup);
                }
            };
        }
        popup!(audio, self.audio.open.then_some(()), |(), _| self
            .render_audio_menu(cx));
        popup!(track, self.track_menu.clone(), |menu, window| self
            .render_track_menu(menu, window, cx));
        popup!(playlist, self.playlist_menu.clone(), |menu, window| self
            .render_playlist_menu(menu, window, cx));
        popup!(library, self.package.menu, |position, window| self
            .render_library_menu(position, window, cx));
        popup!(deck, self.deck_menu.clone(), |menu, window| self
            .render_deck_menu(menu, window, cx));
        popup!(brand, self.brand_menu.open.then_some(()), |(), window| self
            .render_brand_menu(window, cx));
        popup!(
            remove_playlist,
            self.confirm_remove_playlist.clone(),
            |target, _| self.render_remove_playlist_confirm(target, cx)
        );
        popup!(
            import,
            self.show_import_modal.then_some(()),
            |(), window| self.render_import_modal(window, cx)
        );
        popup!(
            export,
            self.package.export_open.then_some(()),
            |(), window| self.render_package_export(window, cx)
        );
        popup!(shortcuts, self.show_shortcuts.then_some(()), |(), _| Some(
            self.render_shortcuts(cx)
        ));
        popup!(fx, self.fx_editor, |(deck, slot), _| Some(
            self.render_fx_editor(deck, slot, cx)
        ));
        popup!(
            master,
            self.master_mixer.open.then_some(()),
            |(), window| Some(self.render_master_mixer(window.viewport_size(), cx))
        );
        motion::stack(popups)
    }
}
