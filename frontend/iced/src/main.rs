use iced::{
    executor, widget::Column, Application, Command, Element, Settings, Subscription, Theme,
};
use iced_aw::{Icon, TabLabel, Tabs};
use reusable::{containers, fonts};
use serial_keel::{
    events::{general, user, TimestampedEvent},
    serial::SerialMessage,
    user::User,
};
use servers::{ServersTab, ServersTabMessage};
use settings::{BarPosition, SettingsTab, SettingsTabMessage};
use user_events::{UserEventsTab, UserEventsTabMessage};

mod subscriptions;

mod reusable;
mod servers;
mod settings;
mod user_events;

fn main() -> iced::Result {
    SerialKeelFrontend::run(Settings::default())
}

struct SerialKeelFrontend {
    // generate_fake_user_events: bool,
    active_tab: usize,
    servers_tab: ServersTab,
    user_events_tab: UserEventsTab,
    settings_tab: SettingsTab,
}

#[derive(Debug, Clone)]
enum Message {
    TabSelected(usize),
    SerialKeelEvent(serial_keel::events::TimestampedEvent),
    ServersTab(ServersTabMessage),
    UserEventsTab(UserEventsTabMessage),
    SettingsTab(SettingsTabMessage),
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
                servers_tab: ServersTab::new(),
                user_events_tab: UserEventsTab::new(),
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
            Message::SerialKeelEvent(event) => match event.inner {
                serial_keel::events::Event::User(user_event) => {
                    return self
                        .user_events_tab
                        .update(UserEventsTabMessage::UserEvent((
                            user_event,
                            event.timestamp,
                        )))
                        .map(Message::UserEventsTab);
                }
                serial_keel::events::Event::General(general_event) => {
                    dbg!(general_event);
                }
            },
            Message::ServersTab(message) => self.servers_tab.update(message),
            Message::UserEventsTab(message) => {
                return self
                    .user_events_tab
                    .update(message)
                    .map(Message::UserEventsTab)
            }
            Message::SettingsTab(message) => self.settings_tab.update(message),
        }

        Command::none()
    }

    fn view(&self) -> Element<Message> {
        let position = self.settings_tab.settings().bar_position();
        let theme = self.settings_tab.settings().bar_theme();

        Tabs::new(self.active_tab, Message::TabSelected)
            .push(self.servers_tab.tab_label(), self.servers_tab.view())
            .push(
                self.user_events_tab.tab_label(),
                self.user_events_tab.view(),
            )
            .push(self.settings_tab.tab_label(), self.settings_tab.view())
            .tab_bar_style(theme)
            .icon_font(fonts::ICONS)
            .tab_bar_position(match position {
                BarPosition::Top => iced_aw::TabBarPosition::Top,
                BarPosition::Bottom => iced_aw::TabBarPosition::Bottom,
            })
            .into()
    }

    fn subscription(&self) -> Subscription<Message> {
        Subscription::batch(vec![
            iced::time::every(std::time::Duration::from_millis(100)).map(|_| {
                Message::SerialKeelEvent(TimestampedEvent::new_user_event(
                    &User::new("John"),
                    user::Event::Connected,
                ))
            }),
            iced::time::every(std::time::Duration::from_millis(250)).map(|_| {
                Message::SerialKeelEvent(TimestampedEvent::new_user_event(
                    &User::new("Mary"),
                    user::Event::Disconnected,
                ))
            }),
            iced::time::every(std::time::Duration::from_millis(500)).map(|_| {
                Message::SerialKeelEvent(TimestampedEvent::new_user_event(
                    &User::new("Joseph"),
                    user::Event::Connected,
                ))
            }),
        ])
    }
}

trait Tab {
    type Message: Clone;

    fn title(&self) -> String;

    fn tab_icon(&self) -> Icon;

    fn tab_label(&self) -> TabLabel {
        TabLabel::IconText(self.tab_icon().into(), self.title())
    }

    fn content(&self) -> Element<'_, Self::Message>;

    fn view(&self) -> Element<'_, Self::Message> {
        let column = Column::new().spacing(20).push(self.content());

        containers::fill_centered_xy(column)
    }
}
