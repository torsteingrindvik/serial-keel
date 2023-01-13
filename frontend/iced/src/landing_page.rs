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
        String::from("Landing Page")
    }

    fn tab_icon(&self) -> crate::Icon {
        Icon::User
    }

    fn content(&self) -> Element<Message> {
        let content: Element<LandingPageMessage> = Text::new("Landing Page").size(50).into();

        content.map(Message::LandingPage)
    }
}
