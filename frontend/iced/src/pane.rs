use iced::theme::{self, Button};
use iced::widget::{container, pane_grid, row, scrollable, text, PaneGrid};
use iced::{widget::Text, Element};
use iced::{Color, Length};
use iced_lazy::responsive;

use crate::{Icon, Message, Tab};

pub struct PaneTab {
    panes: pane_grid::State<Pane>,
    focus: Option<pane_grid::Pane>,
}

struct Pane {
    id: usize,
}

impl Pane {
    fn new(id: usize) -> Self {
        Self { id }
    }
}

impl PaneTab {
    pub fn new() -> Self {
        let (panes, _) = pane_grid::State::new(Pane::new(0));

        Self { focus: None, panes }
    }

    pub fn update(&mut self, message: PaneMessage) {
        match message {
            PaneMessage::Clicked(_) => {
                println!("CLICKLY")
            }
        }
    }
}

#[derive(Debug, Clone)]
pub enum PaneMessage {
    // Inner(pane_grid::)
    Clicked(pane_grid::Pane),
}

impl Tab for PaneTab {
    type Message = Message;

    fn title(&self) -> String {
        String::from("Pane Page")
    }

    fn tab_icon(&self) -> crate::Icon {
        Icon::Heart
    }

    fn content(&self) -> Element<Message> {
        let pane_grid: Element<PaneMessage> =
            PaneGrid::new(&self.panes, |id, pane, is_maximized| {
                let is_focused = self.focus == Some(id);

                let title = row![
                    "Pane",
                    text(pane.id.to_string()).style(if is_focused {
                        PANE_ID_COLOR_FOCUSED
                    } else {
                        PANE_ID_COLOR_UNFOCUSED
                    }),
                ];

                let title_bar = pane_grid::TitleBar::new(title);

                pane_grid::Content::new(responsive(move |size| view_content(id)))
                    .title_bar(title_bar)
            })
            .width(Length::Fill)
            .height(Length::Fill)
            .on_click(PaneMessage::Clicked)
            .into();

        container(pane_grid.map(Message::Pane))
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }
}

fn view_content<'a>(pane: pane_grid::Pane) -> Element<'a, PaneMessage> {
    let content = text("Pane Content");

    container(scrollable(content))
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

const PANE_ID_COLOR_UNFOCUSED: Color = Color::from_rgb(
    0xFF as f32 / 255.0,
    0xC7 as f32 / 255.0,
    0xC7 as f32 / 255.0,
);
const PANE_ID_COLOR_FOCUSED: Color = Color::from_rgb(
    0xFF as f32 / 255.0,
    0x47 as f32 / 255.0,
    0x47 as f32 / 255.0,
);
