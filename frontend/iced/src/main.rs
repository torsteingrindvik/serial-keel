use std::net::Ipv4Addr;

use iced::{
    executor, widget::Column, Application, Command, Element, Settings, Subscription, Theme,
};
use iced_aw::{Icon, TabLabel, Tabs};
use reusable::{containers, fonts};
use serial_keel::{
    events::{user, TimestampedEvent},
    user::User,
};
use servers::{ServerId, ServersTab, ServersTabMessage};
use settings::{BarPosition, SettingsTab, SettingsTabMessage};
use tracing::info;
use user_events::{UserEventsTab, UserEventsTabMessage};

mod subscriptions;

mod reusable;
mod servers;
mod settings;
mod user_events;

fn main() -> iced::Result {
    tracing_subscriber::fmt().init();
    SerialKeelFrontend::run(Settings::default())
}

struct SerialKeelFrontend {
    active_tab: usize,
    servers_tab: ServersTab,
    user_events_tab: UserEventsTab,
    settings_tab: SettingsTab,

    servers: Vec<ServerId>,
}

#[derive(Debug, Clone)]
enum Message {
    TabSelected(usize),
    ServersTab(ServersTabMessage),
    UserEventsTab(UserEventsTabMessage),
    SettingsTab(SettingsTabMessage),
    SerialKeel(ServerId, servers::Event),
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
                servers: vec![],
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
            Message::ServersTab(message) => {
                if let ServersTabMessage::TryConnect(server_id) = &message {
                    self.servers.push(*server_id);
                }

                self.servers_tab.update(message)
            }
            Message::UserEventsTab(message) => {
                return self
                    .user_events_tab
                    .update(message)
                    .map(Message::UserEventsTab)
            }
            Message::SettingsTab(message) => self.settings_tab.update(message),
            Message::SerialKeel(server_id, event) => match event {
                servers::Event::Timestamped(e) => match e.inner {
                    serial_keel::events::Event::User(user_event) => {
                        return self
                            .user_events_tab
                            .update(UserEventsTabMessage::UserEvent((user_event, e.timestamp)))
                            .map(Message::UserEventsTab);
                    }
                    serial_keel::events::Event::General(general_event) => {
                        dbg!(general_event);
                    }
                },
                servers::Event::Error(e) => {
                    info!("Removing {server_id:?} due to {e}");
                    let index = self.servers.iter().position(|id| &server_id == id).unwrap();
                    self.servers.remove(index);
                }
            },
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
        let mut subscriptions = vec![];

        subscriptions.extend(vec![
            iced::time::every(std::time::Duration::from_millis(100)).map(|_| {
                Message::SerialKeel(
                    ServerId::mock(),
                    servers::Event::Timestamped(TimestampedEvent::new_user_event(
                        &User::new("John"),
                        user::Event::Connected,
                    )),
                )
            }),
            iced::time::every(std::time::Duration::from_millis(250)).map(|_| {
                Message::SerialKeel(
                    ServerId::mock(),
                    servers::Event::Timestamped(TimestampedEvent::new_user_event(
                        &User::new("Mary"),
                        user::Event::Disconnected,
                    )),
                )
            }),
            iced::time::every(std::time::Duration::from_millis(500)).map(|_| {
                Message::SerialKeel(
                    ServerId::mock(),
                    servers::Event::Timestamped(TimestampedEvent::new_user_event(
                        &User::new("Greg"),
                        user::Event::Disconnected,
                    )),
                )
            }),
        ]);

        // Servers.
        subscriptions.extend(self.servers.iter().map(|server_id| {
            servers::connect(*server_id).map(|(id, e)| Message::SerialKeel(id, e))
        }));

        Subscription::batch(subscriptions)
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
