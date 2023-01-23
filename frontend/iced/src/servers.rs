use std::{collections::HashMap, net::IpAddr};

use iced::{
    alignment::Horizontal,
    widget::{
        button, column, container,
        pane_grid::{self, Axis},
        scrollable, text, text_input, Button, PaneGrid, Row, Text,
    },
    Element, Length,
};
use iced_aw::{Card, Modal, TabLabel, Tabs};
use serial_keel::{
    client::{DateTime, Utc},
    config::Config,
};

use crate::{
    reusable::{self, elements},
    Icon, Message, Tab,
};

type SharedState = reusable::state::SharedState<SharedServersState>;

#[derive(Debug, Hash, PartialEq, Eq, Clone, Copy)]
pub struct ServerId {
    address: IpAddr,
    port: u16,
}

#[derive(Debug, Clone)]
pub struct ConnectionInfo {
    last_heard_from: DateTime<Utc>,
    last_outgoing_message: DateTime<Utc>,
    alive: bool,
}

#[derive(Debug, Clone)]
pub struct Server {
    config: Config,
    connection: ConnectionInfo,
}

impl Server {
    fn new_mock() -> Self {
        Self {
            config: Config::default(),
            connection: ConnectionInfo {
                last_heard_from: Utc::now(),
                last_outgoing_message: Utc::now(),
                alive: true,
            },
        }
    }

    fn view(&self) -> Element<'_, ServersTabMessage> {
        elements::empty("hi")
    }
}

#[derive(Debug, Default)]
struct SharedServersState {
    servers: HashMap<ServerId, Server>,
    scroll_ids: HashMap<ServerId, scrollable::Id>,
    selected_server: Option<ServerId>,
}

impl SharedServersState {
    fn add_server(&mut self, id: ServerId, server: Server) {
        assert!(self.servers.insert(id, server).is_none());
        self.scroll_ids.insert(id, scrollable::Id::unique());
    }

    fn select_server(&mut self, id: ServerId) {
        self.selected_server = Some(id);
    }
}

#[derive(Debug)]
enum ServersPaneVariant {
    ServersList,
    ServerInfo,
}

#[derive(Debug)]
struct ServersPaneState {
    variant: ServersPaneVariant,
    state: SharedState,
}

// struct RoundedButton<'a, T>(widget::Button<'a, T>);

// impl<'a, T> button::StyleSheet for RoundedButton<'a, T> {
//     type Style = theme::Theme;

//     fn active(&self, button: &theme::Theme) -> button::Appearance {
//         let style = button.style().clone();
//         let appearance = button.active(self.0.style(style));

//         todo!()
//     }
// }

impl ServersPaneState {
    fn new(variant: ServersPaneVariant, state: SharedState) -> Self {
        Self { variant, state }
    }

    fn no_servers(&self) -> bool {
        self.state.state().servers.is_empty()
    }

    fn view_servers_list(&self) -> Element<ServersTabMessage> {
        let e = reusable::containers::fill(text("TODO: Server stuff").size(32));

        let button: Element<ServersTabMessage> = container(
            button(text("Connect").size(32))
                .padding([10, 20])
                .on_press(ServersTabMessage::OpenConnectModal),
        )
        .width(Length::Fill)
        .height(Length::Shrink)
        .center_x()
        .padding(20)
        .into();

        reusable::containers::fill(column![e, button])
    }

    fn view_server_info<'a>(&self) -> Element<'a, ServersTabMessage> {
        if self.no_servers() {
            return elements::empty("No servers");
        }

        let state = self.state.state();
        let Some(server_id) = state.selected_server.as_ref() else {
            return elements::empty("No server selected");
        };

        let server = state.servers.get(server_id).unwrap();

        elements::empty("hi")
    }
}

#[derive(Debug, Default, Clone)]
struct ServerConnectState {
    ip: Option<String>,
    ip_valid: bool,

    port: Option<String>,
    port_valid: bool,
}

impl ServerConnectState {
    fn set_ip(&mut self, ip: Option<String>) {
        self.ip_valid = ip.as_ref().map_or(false, |ip| ip.parse::<IpAddr>().is_ok());
        self.ip = ip;
    }

    fn set_port(&mut self, port: Option<String>) {
        self.port_valid = port.as_ref().map_or(false, |ip| ip.parse::<u16>().is_ok());
        self.port = port;
    }

    fn port_id_valid(&self) -> bool {
        self.ip_valid && self.port_valid
    }
}

pub struct ServersTab {
    // TODO: Move into server info
    active_tab: usize,

    show_connect_modal: bool,
    connect_modal_state: ServerConnectState,

    // The state available to this tab
    shared_state: SharedState,

    // The panes in this tab (server list, and server info).
    // Each pane gets access to the shared state via a new type.
    panes: pane_grid::State<ServersPaneState>,

    focus: Option<pane_grid::Pane>,
}

impl ServersTab {
    pub fn new() -> Self {
        let shared_state: SharedState = Default::default();

        let (mut panes, pane) = pane_grid::State::new(ServersPaneState::new(
            ServersPaneVariant::ServersList,
            shared_state.clone(),
        ));
        let (_, split) = panes
            .split(
                Axis::Vertical,
                &pane,
                ServersPaneState::new(ServersPaneVariant::ServerInfo, shared_state.clone()),
            )
            .unwrap();
        panes.resize(&split, 0.25);

        Self {
            active_tab: 0,
            shared_state,
            panes,
            focus: None,
            show_connect_modal: false,
            connect_modal_state: Default::default(),
        }
    }

    pub fn update(&mut self, message: ServersTabMessage) {
        match message {
            ServersTabMessage::TabSelected(tab) => self.active_tab = tab,
            ServersTabMessage::Clicked(pane) => {
                self.focus = Some(pane);
            }
            ServersTabMessage::Resized(pane_grid::ResizeEvent { split, ratio }) => {
                self.panes.resize(&split, ratio);
            }
            ServersTabMessage::NewServer((id, server)) => {
                self.shared_state.state_mut().add_server(id, server);
            }
            ServersTabMessage::OpenConnectModal => {
                self.show_connect_modal = true;
            }
            ServersTabMessage::CloseConnectModal => {
                self.show_connect_modal = false;
            }
            ServersTabMessage::TryConnect => {
                dbg!(
                    "Want to connect to {}:{}",
                    &self.connect_modal_state.ip,
                    &self.connect_modal_state.port
                );
            }
            ServersTabMessage::ConnectIpChanged(ip) => {
                self.connect_modal_state.set_ip(Some(ip));
            }
            ServersTabMessage::ConnectPortChanged(port) => {
                self.connect_modal_state.set_port(Some(port));
            }
        }
    }
}

#[derive(Debug, Clone)]
pub enum ServersTabMessage {
    OpenConnectModal,
    ConnectIpChanged(String),
    ConnectPortChanged(String),
    TryConnect,
    CloseConnectModal,
    NewServer((ServerId, Server)),
    TabSelected(usize),
    Clicked(pane_grid::Pane),
    Resized(pane_grid::ResizeEvent),
}

impl Tab for ServersTab {
    type Message = Message;

    fn title(&self) -> String {
        String::from("Servers")
    }

    fn tab_icon(&self) -> crate::Icon {
        Icon::Server
    }

    fn content(&self) -> Element<Message> {
        let pane_grid: Element<ServersTabMessage> =
            PaneGrid::new(&self.panes, |pane, state, _is_maximized| {
                let is_focused = self.focus == Some(pane);

                let content = match state.variant {
                    ServersPaneVariant::ServersList => state.view_servers_list(),
                    ServersPaneVariant::ServerInfo => state.view_server_info(),
                };

                pane_grid::Content::new(content).style(if is_focused {
                    reusable::style::pane_focused
                } else {
                    reusable::style::pane_active
                })
            })
            .spacing(5)
            .width(Length::Fill)
            .height(Length::Fill)
            .on_click(ServersTabMessage::Clicked)
            .on_resize(10, ServersTabMessage::Resized)
            .into();

        let content: Element<ServersTabMessage> =
            Tabs::new(self.active_tab, ServersTabMessage::TabSelected)
                .push(TabLabel::Text("Thnks1".into()), text("Noice1"))
                .push(TabLabel::Text("Thnks2".into()), text("Noice2"))
                .push(TabLabel::Text("Thnks3".into()), pane_grid)
                .into();

        let content: Element<ServersTabMessage> =
            Modal::new(self.show_connect_modal, content, move || {
                let button_cancel =
                    Button::new(Text::new("Cancel").horizontal_alignment(Horizontal::Center))
                        .width(Length::Fill)
                        .on_press(ServersTabMessage::CloseConnectModal);

                let mut button_ok =
                    Button::new(Text::new("Ok").horizontal_alignment(Horizontal::Center))
                        .width(Length::Fill);

                let valid = self.connect_modal_state.port_id_valid();

                if valid {
                    button_ok = button_ok.on_press(ServersTabMessage::TryConnect);
                }

                let card_header = Text::new("Connect to server");

                let card_body = Row::new()
                    .push(text_input(
                        "IP",
                        self.connect_modal_state.ip.as_deref().unwrap_or_default(),
                        ServersTabMessage::ConnectIpChanged,
                    ))
                    .push(text_input(
                        "Port",
                        self.connect_modal_state.port.as_deref().unwrap_or_default(),
                        ServersTabMessage::ConnectPortChanged,
                    ))
                    .spacing(10);

                let card_footer = Row::new()
                    .spacing(10)
                    .padding(5)
                    .width(Length::Fill)
                    .push(button_cancel)
                    .push(button_ok);

                Card::new(card_header, card_body)
                    .foot(card_footer)
                    .max_width(300)
                    .on_close(ServersTabMessage::CloseConnectModal)
                    .into()
            })
            .into();

        content.map(Message::ServersTab)
    }

    fn view(&self) -> Element<Message> {
        self.content()
    }
}
