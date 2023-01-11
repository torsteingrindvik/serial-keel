use iced::{widget::Text, Element};

use crate::{Icon, Message, Tab};

pub struct PaneTab {}

impl PaneTab {
    pub fn new() -> Self {
        Self {}
    }

    pub fn update(&mut self, message: PaneMessage) {
        match message {
            PaneMessage::Click => {}
        }
    }
}

#[derive(Debug, Clone)]
pub enum PaneMessage {
    Click,
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
        let content: Element<PaneMessage> = Text::new("Pane Page").size(50).into();

        content.map(Message::Pane)
    }
}
