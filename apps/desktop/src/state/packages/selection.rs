use super::PackageUi;
use mixless_library::PlaylistSummary;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CheckState {
    Off,
    Mixed,
    On,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PackageNode {
    Library,
    Folders,
    Playlists,
    Playlist(i64),
}

impl PackageNode {
    pub fn branch(self) -> Option<usize> {
        match self {
            Self::Library => Some(0),
            Self::Folders => Some(1),
            Self::Playlists => Some(2),
            Self::Playlist(_) => None,
        }
    }

    pub fn includes(self, playlist: &PlaylistSummary) -> bool {
        match self {
            Self::Library => true,
            Self::Folders => playlist.folder_path.is_some(),
            Self::Playlists => playlist.folder_path.is_none(),
            Self::Playlist(id) => playlist.id == id,
        }
    }
}

impl PackageUi {
    pub fn check_state(&self, node: PackageNode, playlists: &[PlaylistSummary]) -> CheckState {
        if self.all {
            return CheckState::On;
        }
        let mut ids = playlists.iter().filter(|p| node.includes(p)).peekable();
        if ids.peek().is_none() {
            return CheckState::Off;
        }
        let mut any = false;
        let mut all = true;
        for playlist in ids {
            let selected = self.selected.contains(&playlist.id);
            any |= selected;
            all &= selected;
        }
        // Selecting every playlist still excludes tracks outside playlists.
        if all && node != PackageNode::Library {
            CheckState::On
        } else if any {
            CheckState::Mixed
        } else {
            CheckState::Off
        }
    }

    pub fn toggle_selection(&mut self, node: PackageNode, playlists: &[PlaylistSummary]) {
        if self.busy() {
            return;
        }
        if node == PackageNode::Library {
            self.all = !self.all;
            self.selected.clear();
            return;
        }
        let remove = self.check_state(node, playlists) == CheckState::On;
        if self.all {
            self.selected = playlists.iter().map(|p| p.id).collect();
            self.all = false;
        }
        for playlist in playlists.iter().filter(|p| node.includes(p)) {
            if remove {
                self.selected.remove(&playlist.id);
            } else {
                self.selected.insert(playlist.id);
            }
        }
    }

    pub fn can_export(&self) -> bool {
        self.all || !self.selected.is_empty()
    }
}
