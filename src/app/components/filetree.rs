use super::{InputOperation, PendingOperation};
use crate::app::component::{Component, Drawable};
use crate::dir::*;
use crate::{
    external_event::ExternalEvent,
    queue::{AppEvent, Queue},
};
use anyhow::Result;
use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use git2::{Repository, Status, Statuses};
use std::{
    cell::Cell,
    path::{Path, PathBuf},
};
use tui::{
    backend::Backend,
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Color, Style},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};
use tui_tree_widget::{Tree, TreeItem, TreeState};

pub struct Filetree {
    state: Cell<TreeState>,
    is_focused: bool,
    dir: Dir,
    root_path: PathBuf,
    queue: Queue,
    only_included: bool,
    repo: Option<Repository>,
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
            only_included: false,
            // TODO: Eventually replace with "." when project root is passed in
            repo: Repository::discover(path).ok(),
        };
        if let Some(item) = tree.get_selected() {
            queue.add(AppEvent::PreviewFile(item.path().to_owned()));
        }
        Ok(tree)
    }

    pub fn refresh(&mut self) -> Result<()> {
        let tree = DirBuilder::new(&self.root_path).dirs_first(true).build()?;
        self.dir = tree;
        self.only_included = false;

        if self.get_selected().is_none() {
            self.state.get_mut().select_first();
        }
        Ok(())
    }

    pub fn get_selected(&self) -> Option<&Item> {
        let state = self.state.take();
        let item = self.dir.nested_child(&state.selected())?;
        self.state.set(state);
        Some(item)
    }

    pub fn only_include(&mut self, include: Vec<PathBuf>) -> Result<()> {
        self.dir = DirBuilder::new(&self.root_path)
            .dirs_first(true)
            .only_include(include)
            .build()?;
        self.only_included = true;

        if self.get_selected().is_none() {
            self.state.get_mut().select_first();
        }
        if let Some(selected) = self.get_selected() {
            self.queue
                .add(AppEvent::PreviewFile(selected.path().to_owned()));
        }
        Ok(())
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
        let items = build_filetree(
            &self.dir,
            self.repo
                .as_ref()
                .and_then(|repo| repo.statuses(None).ok())
                .as_ref(),
        );
        let mut state = self.state.take();

        if self.only_included {
            let layout = Layout::default()
                .constraints([
                    Constraint::Length(1),
                    Constraint::Length(1),
                    Constraint::Min(1),
                ])
                .margin(1)
                .split(area);
            let block = Block::default().borders(Borders::ALL);
            let tree = Tree::new(items)
                .highlight_style(Style::default().fg(Color::Black).bg(Color::LightGreen));
            let p = Paragraph::new("Some results may be filtered out ('\\' to reset)")
                .alignment(Alignment::Center)
                .wrap(Wrap { trim: true })
                .style(Style::default().fg(Color::Yellow));

            f.render_widget(block, area);
            f.render_widget(p, layout[0]);
            f.render_stateful_widget(tree, layout[2], &mut state);
        } else {
            let tree = Tree::new(items)
                .block(Block::default().borders(Borders::ALL))
                .highlight_style(Style::default().fg(Color::Black).bg(Color::LightGreen));
            f.render_stateful_widget(tree, area, &mut state);
        }

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

        let items = build_filetree(&self.dir, None);

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
                    KeyCode::Char('j') if modifiers.is_empty() && !items.is_empty() => {
                        self.state.get_mut().key_down(&items)
                    }
                    KeyCode::Char('k') if modifiers.is_empty() && !items.is_empty() => {
                        self.state.get_mut().key_up(&items)
                    }
                    KeyCode::Char('n')
                        if *modifiers == KeyModifiers::CONTROL && !items.is_empty() =>
                    {
                        for _ in 0..JUMP_DOWN_AMOUNT {
                            self.state.get_mut().key_down(&items);
                        }
                    }
                    KeyCode::Char('p')
                        if *modifiers == KeyModifiers::CONTROL && !items.is_empty() =>
                    {
                        for _ in 0..JUMP_DOWN_AMOUNT {
                            self.state.get_mut().key_up(&items);
                        }
                    }
                    KeyCode::Char('e') if modifiers.is_empty() => {
                        if let Some(item) = self.get_selected() {
                            self.queue.add(AppEvent::OpenInput(InputOperation::Command {
                                to: item.path().to_path_buf(),
                            }))
                        }
                    }
                    KeyCode::Char('d') if modifiers.is_empty() => {
                        if let Some(item) = self.get_selected() {
                            self.queue
                                .add(AppEvent::OpenPopup(PendingOperation::DeleteFile(
                                    item.path().to_path_buf(),
                                )))
                        }
                    }
                    KeyCode::Char('t') if modifiers.is_empty() => {
                        self.queue.add(AppEvent::TogglePreviewMode)
                    }

                    KeyCode::Char('/') if modifiers.is_empty() => self
                        .queue
                        .add(AppEvent::OpenInput(InputOperation::SearchFiles)),
                    KeyCode::Char('\\') if modifiers.is_empty() => self.refresh()?,

                    KeyCode::Enter if modifiers.is_empty() => match self.get_selected() {
                        Some(Item::Dir(_)) => self.state.get_mut().toggle_selected(),
                        Some(Item::File(file)) => self
                            .queue
                            .add(AppEvent::OpenFile(file.path().to_path_buf())),
                        None => {}
                    },
                    KeyCode::Char(key)
                        if (*key == 'n' && modifiers.is_empty())
                            || (*key == 'N' && *modifiers == KeyModifiers::SHIFT) =>
                    {
                        let opened = self.current_is_open();
                        let add_path = match self.get_selected() {
                            // Create new as a child of current selected directory
                            Some(Item::Dir(dir)) if opened => dir.path(),
                            // Create new as a sibling of selected item
                            Some(item) => item.path().parent().expect("item should have parent"),
                            None => return Ok(()),
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
                if let Some(item) = self.get_selected() {
                    self.queue
                        .add(AppEvent::PreviewFile(item.path().to_owned()));
                }
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

fn build_filetree<'a>(tree: &'a Dir, statuses: Option<&Statuses>) -> Vec<TreeItem<'a>> {
    let mut items = Vec::new();
    for item in tree {
        let style = statuses
            .map(|statuses| {
                statuses
                    .iter()
                    .find(|status| {
                        status
                            .path()
                            .map(|path| PathBuf::from(format!("./{path}")) == item.path())
                            // NOTE: Should this be handled differently?
                            .unwrap_or(false)
                    })
                    .map_or(Style::default(), |status| {
                        let status = status.status();
                        match status {
                            Status::WT_NEW => Style::default().fg(Color::Red),
                            Status::WT_MODIFIED => Style::default().fg(Color::Blue),
                            Status::INDEX_MODIFIED | Status::INDEX_NEW => {
                                Style::default().fg(Color::Green)
                            }
                            _ => Style::default(),
                        }
                    })
            })
            // Default style for items, with no git
            .unwrap_or(Style::default());
        let tree_item = match item {
            Item::Dir(dir) => {
                TreeItem::new(last_of_path(dir.path()), build_filetree(dir, statuses)).style(style)
            }
            Item::File(file) => TreeItem::new_leaf(last_of_path(file.path())).style(style),
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
        assert_eq!(
            path.join("test.txt"),
            filetree.get_selected().unwrap().path()
        )
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
        assert_eq!(path.join("test"), filetree.get_selected().unwrap().path());

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

    #[test]
    fn can_send_run_cmd() {
        let temp = temp_files!("test.txt");
        let mut filetree =
            Filetree::from_dir(temp.path(), Queue::new()).expect("should be able to make filetree");
        let path = temp.to_path_buf();
        scopeguard::guard(temp, |temp| temp.close().unwrap());

        let e = input_event!(KeyCode::Char('e'));
        filetree
            .handle_event(&e)
            .expect("should be able to handle event");
        assert!(filetree
            .queue
            .contains(&AppEvent::OpenInput(InputOperation::Command {
                to: path.join("test.txt")
            })))
    }

    #[test]
    fn can_send_search_cmd() {
        let temp = temp_files!();
        let mut filetree = Filetree::from_dir(temp.path(), Queue::new()).unwrap();
        scopeguard::guard(temp, |temp| temp.close().unwrap());

        let slash = input_event!(KeyCode::Char('/'));
        filetree
            .handle_event(&slash)
            .expect("should be able to handle event");
        assert!(filetree
            .queue
            .contains(&AppEvent::OpenInput(InputOperation::SearchFiles)))
    }

    #[test]
    fn can_only_include() {
        let temp = temp_files!("test.txt", "test2.txt");
        let mut filetree = Filetree::from_dir(temp.path(), Queue::new()).unwrap();

        assert!(filetree
            .only_include(vec![temp.path().join("test.txt")])
            .is_ok());
        assert_eq!(1, filetree.dir.iter().len());
        temp.close().unwrap();
    }

    #[test]
    fn can_send_toggle_preview_cmd() {
        let temp = temp_files!();
        let mut filetree = Filetree::from_dir(temp.path(), Queue::new()).unwrap();
        scopeguard::guard(temp, |temp| temp.close().unwrap());

        let slash = input_event!(KeyCode::Char('t'));
        filetree
            .handle_event(&slash)
            .expect("should be able to handle event");
        assert!(filetree.queue.contains(&AppEvent::TogglePreviewMode));
    }
}
