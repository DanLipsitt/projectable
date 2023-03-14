mod filetree;

use anyhow::Result;
use filetree::{Filetree, Item};
use std::path::{Path, PathBuf};

#[derive(Debug, PartialEq, Eq)]
pub enum PendingOperations {
    DeleteFile,
    NoPending,
}

pub struct App<'a> {
    tree: Filetree<'a>,
    path: PathBuf,
    should_quit: bool,
    pub pending: PendingOperations,
}

impl<'a> App<'a> {
    pub fn new(path: impl AsRef<Path>) -> Result<App<'a>> {
        let app = App {
            path: path.as_ref().to_path_buf(),
            tree: Filetree::from_dir(&path)?,
            should_quit: false,
            pending: PendingOperations::NoPending,
        };

        Ok(app)
    }

    pub fn handle_key(&mut self, key: char) -> Result<()> {
        if self.pending != PendingOperations::NoPending {
            if key == 'q' {
                self.complete_pending(false);
            }
            return Ok(());
        }

        match key {
            'q' => self.should_quit = true,

            'g' => self.tree.first(),
            'G' => self.tree.last(),
            'r' => self.tree.refresh()?,
            'd' => self.pending = PendingOperations::DeleteFile,

            // Movement
            'j' => self.on_down(),
            'k' => self.on_up(),
            _ => {}
        }
        Ok(())
    }

    pub fn on_enter(&mut self) -> Result<Option<PathBuf>> {
        if let Some(result) = self.complete_pending(true) {
            return match result {
                Ok(_) => Ok(None),
                Err(err) => Err(err),
            };
        }

        match self.tree.get_selected() {
            Item::Dir(_) => self.tree.toggle(),
            Item::File(file) => return Ok(Some(file.path().to_path_buf())),
        }
        Ok(None)
    }

    pub fn on_esc(&mut self) -> Result<()> {
        if let Some(result) = self.complete_pending(false) {
            return result;
        }

        self.should_quit = true;
        Ok(())
    }

    pub fn on_up(&mut self) {
        self.tree.up();
    }

    pub fn on_down(&mut self) {
        self.tree.down();
    }

    pub fn should_quit(&self) -> bool {
        self.should_quit
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn tree(&self) -> &Filetree<'a> {
        &self.tree
    }

    pub fn tree_mut(&mut self) -> &mut Filetree<'a> {
        &mut self.tree
    }

    fn complete_pending(&mut self, confirmed: bool) -> Option<Result<()>> {
        if self.pending != PendingOperations::NoPending && !confirmed {
            self.pending = PendingOperations::NoPending;
            return Some(Ok(()));
        }
        let res = match self.pending {
            PendingOperations::NoPending => None,
            PendingOperations::DeleteFile => {
                Some(self.tree_mut().remove_selected().and_then(|_| Ok(())))
            }
        };
        self.pending = PendingOperations::NoPending;
        res
    }
}
