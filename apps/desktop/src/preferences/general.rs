use super::*;

impl Preferences {
    pub(super) fn general(&self, wide: bool, cx: &Context<Self>) -> gpui::AnyElement {
        let settings = self.core.settings.get();
        let workspace = setting_group(
            "Workspace",
            vec![
                row(
                    "Waveforms",
                    self.segmented(
                        "layout",
                        &[(WaveLayout::Top, "Top"), (WaveLayout::Center, "Center")],
                        settings.wave_layout,
                        |s, value| s.save_general(|p| p.wave_layout = value),
                        cx,
                    ),
                ),
                row(
                    "Effects panel",
                    self.toggle(
                        "show-fx",
                        settings.show_fx,
                        |s| s.save_general(|p| p.show_fx = !p.show_fx),
                        cx,
                    ),
                ),
            ],
        );
        let decks = setting_group(
            "Deck playback",
            vec![
                row(
                    "Quantize",
                    self.toggle(
                        "quantize",
                        settings.quantize,
                        |s| s.save_general(|p| p.quantize = !p.quantize),
                        cx,
                    ),
                ),
                row(
                    "Key lock",
                    self.toggle(
                        "keylock",
                        settings.keylock,
                        |s| s.save_general(|p| p.keylock = !p.keylock),
                        cx,
                    ),
                ),
                row(
                    "Vinyl mode",
                    self.toggle(
                        "vinyl",
                        settings.vinyl,
                        |s| s.save_general(|p| p.vinyl = !p.vinyl),
                        cx,
                    ),
                ),
                row(
                    "Slip mode",
                    self.toggle(
                        "slip",
                        settings.slip,
                        |s| s.save_general(|p| p.slip = !p.slip),
                        cx,
                    ),
                ),
            ],
        );
        let mixer = setting_group(
            "Mixer",
            vec![
                row(
                    "FX fade in / out",
                    self.toggle(
                        "fx-auto-fade",
                        settings.fx_auto_fade,
                        |s| s.save_general(|p| p.fx_auto_fade = !p.fx_auto_fade),
                        cx,
                    ),
                ),
                row(
                    "Filter / EQ resonance",
                    self.toggle(
                        "filter-resonance",
                        settings.filter_resonance,
                        |s| s.save_general(|p| p.filter_resonance = !p.filter_resonance),
                        cx,
                    ),
                ),
                row(
                    "Crossfader reverse",
                    self.toggle(
                        "xf-reverse",
                        settings.xf_reverse,
                        |s| s.save_general(|p| p.xf_reverse = !p.xf_reverse),
                        cx,
                    ),
                ),
                div()
                    .flex()
                    .flex_col()
                    .items_start()
                    .gap_2()
                    .py_3()
                    .child(div().text_size(px(12.)).child("Crossfader curve"))
                    .child(self.segmented(
                        "curve",
                        &[
                            (XfCurve::Linear, "Linear"),
                            (XfCurve::EqualPower, "Equal power"),
                            (XfCurve::Cut, "Cut"),
                            (XfCurve::Scratch, "Scratch"),
                        ],
                        settings.xf_curve,
                        |s, value| s.save_general(|p| p.xf_curve = value),
                        cx,
                    )),
            ],
        );
        let automix = setting_group(
            "AutoMix",
            vec![
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .py_3()
                    .child(
                        row(
                            "Live moves",
                            self.segmented(
                                "moves",
                                &[
                                    (LiveMoves::Off, "Off"),
                                    (LiveMoves::Subtle, "Subtle"),
                                    (LiveMoves::Active, "Active"),
                                ],
                                settings.live_moves,
                                |s, value| s.save_general(|p| p.live_moves = value),
                                cx,
                            ),
                        )
                        .border_0()
                        .min_h(px(28.))
                        .py_0(),
                    )
                    .child(
                        div()
                            .text_size(px(11.))
                            .line_height(px(16.))
                            .text_color(theme::MUTED)
                            .child("Gentle filter and drum moves. Active also permits spinback accents."),
                    ),
            ],
        );

        let download_dir = self.core.download_dir();
        let folder = crate::state::display_dir(&download_dir);
        let folder_tip = folder.clone();
        let library = setting_group(
            "Library",
            vec![
                setting_detail(
                    "Stem & note analysis",
                    if !mixless_stems::inference_supported() {
                        "This Mac can play stems imported from a package. Separation requires Apple Silicon."
                    } else if self
                        .core
                        .stems
                        .as_ref()
                        .is_some_and(|s| s.has_bundled_models())
                    {
                        "Runs locally with the models included in this app, even offline."
                    } else {
                        "Runs locally in the background. Downloads the required models on first use."
                    },
                    self.toggle(
                        "deep-analysis",
                        settings.deep_analysis,
                        |s| s.save_general(|p| p.deep_analysis = !p.deep_analysis),
                        cx,
                    ),
                ),
                row(
                    "Library & preferences",
                    self.button(
                        "data-folder",
                        "Show in Finder",
                        true,
                        |s, _, cx| {
                            if let Some(dir) = s.core.settings.path.parent() {
                                cx.reveal_path(dir);
                            }
                        },
                        cx,
                    )
                    .into_any_element(),
                ),
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .py_3()
                    .child(
                        row(
                            "Downloaded audio",
                            div()
                                .flex()
                                .gap_2()
                                .child(self.button(
                                    "audio-folder-show",
                                    "Show in Finder",
                                    true,
                                    |s, _, cx| {
                                        let dir = s.core.download_dir();
                                        std::fs::create_dir_all(&dir).ok();
                                        cx.reveal_path(&dir);
                                    },
                                    cx,
                                ))
                                .child(self.button(
                                    "audio-folder-change",
                                    "Change…",
                                    self.job.is_none(),
                                    |s, _, cx| s.pick_download_dir(cx),
                                    cx,
                                ))
                                .when(settings.download_dir.is_some(), |el| {
                                    el.child(self.button(
                                        "audio-folder-default",
                                        "Use default",
                                        self.job.is_none(),
                                        |s, _, _| s.save_general(|p| p.download_dir = None),
                                        cx,
                                    ))
                                })
                                .into_any_element(),
                        )
                        .border_0()
                        .min_h(px(30.))
                        .py_0(),
                    )
                    .child(
                        div()
                            .id("download-folder-path")
                            .w_full()
                            .h(px(16.))
                            .text_size(px(11.))
                            .text_color(theme::MUTED)
                            .child(crate::views::ellipsized_text(folder).w_full().h_full())
                            .tooltip(move |_, cx| {
                                cx.new(|_| crate::views::TextTip(folder_tip.clone())).into()
                            }),
                    )
                    .child(
                        div()
                            .text_size(px(11.))
                            .line_height(px(16.))
                            .text_color(theme::MUTED)
                            .child("Spotify downloads are grouped in a folder for each playlist."),
                    ),
            ],
        );

        setting_stack()
            .w_full()
            .max_w(px(880.))
            .mx_auto()
            .py_5()
            .child(
                div()
                    .flex()
                    .gap(px(20.))
                    .when(!wide, |el| el.flex_col())
                    .child(setting_stack().flex_1().child(workspace).child(decks))
                    .child(setting_stack().flex_1().child(mixer).child(automix)),
            )
            .child(library)
            .into_any_element()
    }
}
