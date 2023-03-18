use super::{InputOperation, PendingOperation};
use crate::app::component::{Component, Drawable};
use crate::dir::*;
use crate::{
    external_event::ExternalEvent,
    queue::{AppEvent, Queue},
};
use anyhow::Result;
use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use std::{
    cell::Cell,
    path::{Path, PathBuf},
};
use tui::{
    backend::Backend,
    layout::Rect,
    style::{Color, Style},
    widgets::{Block, Borders},
    Frame,
};
use tui_tree_widget::{Tree, TreeItem, TreeState};

pub struct Filetree {
    state: Cell<TreeState>,
    is_focused: bool,
    dir: Dir,
    root_path: PathBuf,
    queue: Queue,
}

impl Filetree {
    pub fn from_dir(path: impl AsRef<Path>, queue: Queue) -> Result<Self> {
        let tree = DirBuilder::new(&path).dirs_first(true).build()?;
        let mut state = TreeState::default();
        state.select_first();
        let tree = Filetree {
            root_path: path.as_ref().to_path_buf(),
            state: state.into(),
            is_focused: true,
            queue: queue.clone(),
            dir: tree,
        };
        queue.add(AppEvent::PreviewFile(tree.get_selected().path().to_owned()));
        Ok(tree)
    }

    pub fn refresh(&mut self) -> Result<()> {
        let tree = DirBuilder::new(&self.root_path).dirs_first(true).build()?;
        self.dir = tree;

        if self
            .dir
            .nested_child(&self.state.get_mut().selected())
            .is_none()
        {
            self.state.get_mut().select_first();
        }
        Ok(())
    }

    pub fn get_selected(&self) -> &Item {
        let state = self.state.take();
        let item = self
            .dir
            .nested_child(&state.selected())
            .expect("selected item should be in tree");
        self.state.set(state);
        item
    }

    fn current_is_open(&mut self) -> bool {
        let selected = self.state.get_mut().selected();
        // Will return true if it was already closed
        let closed = self.state.get_mut().open(selected.clone());
        if closed {
            // Reverse the opening
            self.state.get_mut().close(&selected);
        }
        !closed
    }
}

impl Drawable for Filetree {
    fn draw<B: Backend>(&self, f: &mut Frame<B>, area: Rect) -> Result<()> {
        let items = build_filetree(&self.dir);
        let tree = Tree::new(items)
            .block(Block::default().borders(Borders::ALL))
            .highlight_style(Style::default().fg(Color::Black).bg(Color::LightGreen));

        let mut state = self.state.take();
        f.render_stateful_widget(tree, area, &mut state);
        self.state.set(state);

        Ok(())
    }
}

impl Component for Filetree {
    fn visible(&self) -> bool {
        true
    }

    fn focus(&mut self, focus: bool) {
        self.is_focused = focus;
    }
    fn focused(&self) -> bool {
        self.is_focused
    }

    fn handle_event(&mut self, ev: &ExternalEvent) -> Result<()> {
        if !self.focused() {
            return Ok(());
        }

        let items = build_filetree(&self.dir);

        const JUMP_DOWN_AMOUNT: u8 = 3;
        match ev {
            ExternalEvent::RefreshFiletree => self.refresh()?,
            ExternalEvent::Crossterm(Event::Key(KeyEvent {
                code, modifiers, ..
            })) => {
                let mut refresh_preview = true;
                match code {
                    KeyCode::Char('g') if modifiers.is_empty() => {
                        self.state.get_mut().select_first()
                    }
                    KeyCode::Char('G') if *modifiers == KeyModifiers::SHIFT => {
                        self.state.get_mut().select_last(&items)
                    }
                    KeyCode::Char('j') if modifiers.is_empty() => {
                        self.state.get_mut().key_down(&items)
                    }
                    KeyCode::Char('n') if *modifiers == KeyModifiers::CONTROL => {
                        for _ in 0..JUMP_DOWN_AMOUNT {
                            self.state.get_mut().key_down(&items);
                        }
                    }
                    KeyCode::Char('p') if *modifiers == KeyModifiers::CONTROL => {
                        for _ in 0..JUMP_DOWN_AMOUNT {
                            self.state.get_mut().key_up(&items);
                        }
                    }
                    KeyCode::Char('k') if modifiers.is_empty() => {
                        self.state.get_mut().key_up(&items)
                    }
                    KeyCode::Char('d') if modifiers.is_empty() => {
                        self.queue
                            .add(AppEvent::OpenPopup(PendingOperation::DeleteFile(
                                self.get_selected().path().to_path_buf(),
                            )))
                    }
                    KeyCode::Enter if modifiers.is_empty() => match self.get_selected() {
                        Item::Dir(_) => self.state.get_mut().toggle_selected(),
                        Item::File(file) => self
                            .queue
                            .add(AppEvent::OpenFile(file.path().to_path_buf())),
                    },
                    KeyCode::Char(key)
                        if (*key == 'n' && modifiers.is_empty())
                            || (*key == 'N' && *modifiers == KeyModifiers::SHIFT) =>
                    {
                        let opened = self.current_is_open();
                        let add_path = match self.get_selected() {
                            // Create new as a child of current selected directory
                            Item::Dir(dir) if opened => dir.path(),
                            // Create new as a siblilng of selected item
                            item => item.path().parent().expect("item should have parent"),
                        };
                        let event = if *key == 'n' {
                            AppEvent::OpenInput(InputOperation::NewFile {
                                at: add_path.to_path_buf(),
                            })
                        } else {
                            AppEvent::OpenInput(InputOperation::NewDir {
                                at: add_path.to_path_buf(),
                            })
                        };
                        self.queue.add(event);
                    }
                    _ => refresh_preview = false,
                }
                if !refresh_preview {
                    return Ok(());
                }
                self.queue
                    .add(AppEvent::PreviewFile(self.get_selected().path().to_owned()));
            }
            _ => {}
        }

        Ok(())
    }
}

fn last_of_path(path: impl AsRef<Path>) -> String {
    path.as_ref()
        .iter()
        .last()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string()
}

fn build_filetree(tree: &Dir) -> Vec<TreeItem> {
    let mut items = Vec::new();
    for item in tree {
        let tree_item = match item {
            Item::Dir(dir) => TreeItem::new(last_of_path(dir.path()), build_filetree(dir)),
            Item::File(file) => TreeItem::new_leaf(last_of_path(file.path())),
        };
        items.push(tree_item);
    }
    items
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::components::testing::*;

    #[test]
    fn last_of_path_only_gets_last_part() {
        let name = last_of_path("t/d/d/s/test.txt");
        assert_eq!("test.txt".to_owned(), name);
    }

    #[test]
    fn last_of_path_works_with_one_part() {
        let name = last_of_path("test.txt");
        assert_eq!("test.txt", name);
    }

    #[test]
    fn new_filetree_selects_first() {
        let temp = temp_files!("test.txt");
        let path = temp.path().to_owned();
        let filetree =
            Filetree::from_dir(&path, Queue::new()).expect("should be able to make filetree");
        scopeguard::guard(temp, |temp| temp.close().unwrap());
        assert_eq!(path.join("test.txt"), filetree.get_selected().path())
    }

    #[test]
    fn sends_delete_event() {
        let temp = temp_files!("test.txt");
        let path = temp.path().to_owned();
        let mut filetree =
            Filetree::from_dir(&path, Queue::new()).expect("should be able to make filetree");
        scopeguard::guard(temp, |temp| temp.close().unwrap());

        let d = input_event!(KeyCode::Char('d'));
        filetree
            .handle_event(&d)
            .expect("should be able to handle keypress");
        assert!(filetree
            .queue
            .contains(&AppEvent::OpenPopup(PendingOperation::DeleteFile(
                path.join("test.txt")
            ))));
    }

    #[test]
    fn sends_new_file_and_new_dir_events() {
        let temp = temp_files!("test.txt");
        let path = temp.path().to_owned();
        let mut filetree =
            Filetree::from_dir(&path, Queue::new()).expect("should be able to make filetree");
        scopeguard::guard(temp, |temp| temp.close().unwrap());

        let n = input_event!(KeyCode::Char('n'));
        filetree
            .handle_event(&n)
            .expect("should be able to handle keypress");
        assert!(filetree
            .queue
            .contains(&AppEvent::OpenInput(InputOperation::NewFile {
                at: path.clone()
            })));

        let caps_n = input_event!(KeyCode::Char('N'); KeyModifiers::SHIFT);
        filetree
            .handle_event(&caps_n)
            .expect("should be able to handle keypress");
        assert!(filetree
            .queue
            .contains(&AppEvent::OpenInput(InputOperation::NewDir { at: path })));
    }

    #[test]
    fn makes_new_file_as_sibling_when_selected_dir_is_closed() {
        let temp = temp_files!("test/test.txt");
        let path = temp.path().to_owned();
        let mut filetree =
            Filetree::from_dir(&path, Queue::new()).expect("should be able to make filetree");
        scopeguard::guard(temp, |temp| temp.close().unwrap());
        assert_eq!(path.join("test"), filetree.get_selected().path());

        let n = input_event!(KeyCode::Char('n'));
        filetree
            .handle_event(&n)
            .expect("should be able to handle keypress");
        assert!(filetree
            .queue
            .contains(&AppEvent::OpenInput(InputOperation::NewFile { at: path })));
    }

    #[test]
    fn makes_new_file_as_child_when_selected_dir_is_open() {
        let temp = temp_files!("test/test.txt");
        let path = temp.path().to_owned();
        let mut filetree =
            Filetree::from_dir(&path, Queue::new()).expect("should be able to make filetree");
        filetree.state.get_mut().toggle_selected();
        scopeguard::guard(temp, |temp| temp.close().unwrap());

        let n = input_event!(KeyCode::Char('n'));
        filetree
            .handle_event(&n)
            .expect("should be able to handle keypress");
        assert!(filetree
            .queue
            .contains(&AppEvent::OpenInput(InputOperation::NewFile {
                at: path.join("test")
            })));
    }

    #[test]
    fn enter_opens_when_over_dir() {
        let temp = temp_files!("test/test.txt");
        let mut filetree =
            Filetree::from_dir(temp.path(), Queue::new()).expect("should be able to make filetree");
        scopeguard::guard(temp, |temp| temp.close().unwrap());

        let enter = input_event!(KeyCode::Enter);
        filetree
            .handle_event(&enter)
            .expect("should be able to handle keypress");
        assert_eq!(vec![vec![0]], filetree.state.get_mut().get_all_opened());
    }

    #[test]
    fn enter_sends_open_file_when_over_files() {
        let temp = temp_files!("test.txt");
        let path = temp.path().to_owned();
        let mut filetree =
            Filetree::from_dir(&path, Queue::new()).expect("should be able to make filetree");
        scopeguard::guard(temp, |temp| temp.close().unwrap());

        let enter = input_event!(KeyCode::Enter);
        filetree
            .handle_event(&enter)
            .expect("should be able to handle keypress");
        assert!(filetree
            .queue
            .contains(&AppEvent::OpenFile(path.join("test.txt"))));
    }

    #[test]
    fn can_jump_down_by_three() {
        let temp = temp_files!("test.txt", "test2.txt", "test3.txt", "test4.txt");
        let mut filetree =
            Filetree::from_dir(temp.path(), Queue::new()).expect("should be able to make filetree");
        scopeguard::guard(temp, |temp| temp.close().unwrap());

        let ctrl_n = input_event!(KeyCode::Char('n'); KeyModifiers::CONTROL);
        filetree
            .handle_event(&ctrl_n)
            .expect("should be able to handle keypress");
        assert_eq!(3, filetree.state.get_mut().selected()[0])
    }

    #[test]
    fn can_jump_up_by_three() {
        let temp = temp_files!("test.txt", "test2.txt", "test3.txt", "test4.txt");
        let mut filetree =
            Filetree::from_dir(temp.path(), Queue::new()).expect("should be able to make filetree");
        scopeguard::guard(temp, |temp| temp.close().unwrap());

        let inputs = input_events!(KeyCode::Char('G'); KeyModifiers::SHIFT, KeyCode::Char('p'); KeyModifiers::CONTROL);
        for input in inputs {
            filetree
                .handle_event(&input)
                .expect("should be able to handle keypress");
        }
        assert_eq!(0, filetree.state.get_mut().selected()[0])
    }
}
