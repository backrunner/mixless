//! Explicit order choices belong to playlist identities, including All tracks.
use std::collections::HashMap;

pub(super) use crate::automix::OrderMode as Mode;

#[derive(Default)]
pub(crate) struct OrderSelection {
    source: Option<Option<i64>>,
    choices: HashMap<Option<i64>, Mode>,
    pub(super) pending_cache: bool,
}

impl OrderSelection {
    /// The outer option indicates a source change; the inner option is its
    /// explicit mode, if any. Never reset a choice on a same-source refresh.
    pub(super) fn select(&mut self, source: Option<i64>) -> Option<Option<Mode>> {
        if self.source == Some(source) {
            return None;
        }
        self.source = Some(source);
        let choice = self.choices.get(&source).copied();
        self.pending_cache = choice.is_none() || choice == Some(Mode::Best);
        Some(choice)
    }

    pub(super) fn choose(&mut self, source: Option<i64>, mode: Mode) {
        self.source = Some(source);
        self.choices.insert(source, mode);
        self.pending_cache = false;
    }

    pub(super) fn prefers_best(&self) -> bool {
        self.choice() == Some(Mode::Best)
    }

    pub(super) fn choice(&self) -> Option<Mode> {
        self.source
            .and_then(|source| self.choices.get(&source).copied())
    }
}
