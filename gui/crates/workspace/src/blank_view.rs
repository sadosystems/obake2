//! obake: a placeholder pane item. It has a tab, so it can be dragged, split, moved
//! between panes and docked like any other item, which keeps the tiling layout usable
//! while panes have nothing else to show. Real obake views (frames, timelines, traces)
//! replace it later.

use gpui::{
    App, Context, Entity, EventEmitter, FocusHandle, Focusable, IntoElement, ParentElement,
    Render, SharedString, Styled, Task, Window, div,
};
use ui::{Color, Label, LabelCommon as _, prelude::*};

use crate::{Item, WorkspaceId, item::ItemEvent};

pub struct BlankView {
    focus_handle: FocusHandle,
}

impl BlankView {
    pub fn new(cx: &mut App) -> Entity<Self> {
        cx.new(|cx| Self {
            focus_handle: cx.focus_handle(),
        })
    }
}

impl EventEmitter<ItemEvent> for BlankView {}

impl Focusable for BlankView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Item for BlankView {
    type Event = ItemEvent;

    fn tab_content_text(&self, _detail: usize, _: &App) -> SharedString {
        "Empty".into()
    }

    fn to_item_events(event: &Self::Event, f: &mut dyn FnMut(ItemEvent)) {
        f(*event)
    }

    fn can_split(&self) -> bool {
        true
    }

    fn clone_on_split(
        &self,
        _workspace_id: Option<WorkspaceId>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Task<Option<Entity<Self>>> {
        Task::ready(Some(BlankView::new(cx)))
    }
}

impl Render for BlankView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            // Match the active tab, which is drawn in the editor background color.
            .bg(cx.theme().colors().editor_background)
            .track_focus(&self.focus_handle(cx))
            .flex()
            .items_center()
            .justify_center()
            .child(Label::new("Empty").color(Color::Muted))
    }
}
