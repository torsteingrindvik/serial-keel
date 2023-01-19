use iced::{widget::Text, Element};

use crate::{Icon, Message, Tab};

pub struct LandingPageTab {}

impl LandingPageTab {
    pub fn new() -> Self {
        Self {}
    }

    pub fn update(&mut self, message: LandingPageMessage) {
        match message {}
    }
}

#[derive(Debug, Clone)]
pub enum LandingPageMessage {}

impl Tab for LandingPageTab {
    type Message = Message;

    fn title(&self) -> String {
        String::from("Servers")
    }

    fn tab_icon(&self) -> crate::Icon {
        Icon::Server
    }

    fn content(&self) -> Element<Message> {
        let content: Element<LandingPageMessage> = Text::new("TODO: Servers").size(50).into();

        content.map(Message::LandingPage)
    }
}
