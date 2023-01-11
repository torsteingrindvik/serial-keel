use iced::{
    alignment, executor,
    widget::{Column, Container, Text},
    Application, Command, Element, Font, Length, Settings, Theme,
};
use iced_aw::{TabLabel, Tabs};
use landing_page::{LandingPageMessage, LandingPageTab};
use pane::{PaneMessage, PaneTab};
use scrollable::{ScrollableMessage, ScrollableTab};
use settings::{BarPosition, SettingsMessage, SettingsTab};

mod landing_page;
mod pane;
mod scrollable;
mod settings;

const HEADER_SIZE: u16 = 32;
const TAB_PADDING: u16 = 16;

const ICON_FONT: Font = Font::External {
    name: "Icons",
    bytes: include_bytes!("../assets/fonts/icons.ttf"),
};

enum Icon {
    User,
    Heart,
    Calc,
    CogAlt,
}

impl From<Icon> for char {
    fn from(icon: Icon) -> Self {
        match icon {
            // TODO: Lookup these
            Icon::User => '\u{E800}',
            Icon::Heart => '\u{E801}',
            Icon::Calc => '\u{F1EC}',
            Icon::CogAlt => '\u{E802}',
        }
    }
}

fn main() -> iced::Result {
    SerialKeelFrontend::run(Settings::default())
}

struct SerialKeelFrontend {
    active_tab: usize,
    landing_page_tab: LandingPageTab,
    pane_tab: PaneTab,
    scrollable_tab: ScrollableTab,
    settings_tab: SettingsTab,
}

#[derive(Debug, Clone)]
enum Message {
    TabSelected(usize),
    LandingPage(LandingPageMessage),
    Pane(PaneMessage),
    Scrollable(ScrollableMessage),
    Settings(SettingsMessage),
}

impl Application for SerialKeelFrontend {
    type Executor = executor::Default;
    type Message = Message;
    type Theme = Theme;
    type Flags = ();

    fn new(_flags: ()) -> (Self, Command<Message>) {
        (
            Self {
                active_tab: 0,
                landing_page_tab: LandingPageTab::new(),
                pane_tab: PaneTab::new(),
                scrollable_tab: ScrollableTab::new(),
                settings_tab: SettingsTab::new(),
            },
            Command::none(),
        )
    }

    fn title(&self) -> String {
        String::from("Serial Keel Frontend")
    }

    fn update(&mut self, message: Message) -> Command<Message> {
        match message {
            Message::TabSelected(selected) => self.active_tab = selected,
            Message::LandingPage(message) => self.landing_page_tab.update(message),
            Message::Pane(message) => self.pane_tab.update(message),
            Message::Scrollable(message) => self.scrollable_tab.update(message),
            Message::Settings(message) => self.settings_tab.update(message),
        }

        Command::none()
    }

    fn view(&self) -> Element<Message> {
        let position = self.settings_tab.settings().bar_position();
        let theme = self.settings_tab.settings().bar_theme();

        Tabs::new(self.active_tab, Message::TabSelected)
            .push(
                self.landing_page_tab.tab_label(),
                self.landing_page_tab.view(),
            )
            .push(self.pane_tab.tab_label(), self.pane_tab.view())
            .push(self.scrollable_tab.tab_label(), self.scrollable_tab.view())
            .push(self.settings_tab.tab_label(), self.settings_tab.view())
            .tab_bar_style(theme)
            .icon_font(ICON_FONT)
            .tab_bar_position(match position {
                BarPosition::Top => iced_aw::TabBarPosition::Top,
                BarPosition::Bottom => iced_aw::TabBarPosition::Bottom,
            })
            .into()
    }
}

trait Tab {
    type Message;

    fn title(&self) -> String;

    fn tab_icon(&self) -> Icon;

    fn tab_label(&self) -> TabLabel {
        TabLabel::IconText(self.tab_icon().into(), self.title())
    }

    fn content(&self) -> Element<'_, Self::Message>;

    fn view(&self) -> Element<'_, Self::Message> {
        let column = Column::new()
            .spacing(20)
            .push(Text::new(self.title()).size(HEADER_SIZE))
            .push(self.content());

        Container::new(column)
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(alignment::Horizontal::Center)
            .align_y(alignment::Vertical::Center)
            .padding(TAB_PADDING)
            .into()
    }
}
