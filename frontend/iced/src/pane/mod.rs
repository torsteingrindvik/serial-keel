use std::{
    collections::{BTreeMap, HashMap},
    sync::{Arc, RwLock, RwLockWriteGuard},
    vec,
};

use iced::{
    widget::{
        self, container,
        pane_grid::{self, Axis},
        scrollable::{self, snap_to},
        text, PaneGrid, Radio,
    },
    Color, Command, Element, Length,
};
use serial_keel::{client::*, user::User};

use crate::{reusable::container_fill_center, Icon, Message, Tab};

mod views;

// #[derive(Debug)]
// struct Lhs {
//     scroll: scrollable::State,
// }

// #[derive(Debug)]
// struct Rhs {
//     scroll: scrollable::State,
// }

// #[derive(Debug)]
// enum LocalPaneState {
//     Lhs(Lhs),
//     Rhs(Rhs),
// }
#[derive(Debug)]
enum PaneVariant {
    Users,
    UserEvents,
}

#[derive(Debug, Default)]
struct UserEventState {
    events: BTreeMap<User, Vec<(Event, DateTime<Utc>)>>,
    scroll_ids: HashMap<User, scrollable::Id>,
    selected_user: Option<User>,
}

type Events = Vec<(Event, DateTime<Utc>)>;

impl UserEventState {
    fn users(&self) -> Vec<User> {
        self.events.keys().cloned().collect()
    }

    fn events(&self) -> Events {
        match &self.selected_user {
            Some(user) => match self.events.get(user) {
                Some(events) => events.iter().rev().take(100).rev().cloned().collect(),
                None => vec![],
            },
            None => vec![],
        }
    }
}

#[derive(Debug)]
struct PaneState {
    variant: PaneVariant,
    user_events_state: SharedState,
}

impl PaneState {
    fn view_content<'a>(&'a self, time_display: TimeDisplaySetting) -> Element<'a, PaneMessage> {
        let user_events_state = self
            .user_events_state
            .try_read()
            .expect("View should not overlap update");

        match self.variant {
            PaneVariant::Users => views::users::view(user_events_state.users()),
            PaneVariant::UserEvents => {
                let id = user_events_state.selected_user.as_ref().map(|user| {
                    user_events_state
                        .scroll_ids
                        .get(&user)
                        .expect("User should have a scroll id")
                        .clone()
                });

                views::user_events::view(user_events_state.events(), id, time_display)
            }
        }
        // let some_buttons = row![Button::new(text(format!("Hey, I'm {:?}", self.variant)))];

        // let users = column(
        //     (0..100)
        //         .into_iter()
        //         .map(|i| Text::new(format!("User #{}", i)).into())
        //         .collect(),
        // )
        // .width(Length::Fill);

        // let contents = column![some_buttons, users];

        // container(scrollable(contents))
        //     .padding(5)
        //     .width(Length::Fill)
        //     .height(Length::Fill)
        //     .into()
    }
}

impl PaneState {
    fn new(variant: PaneVariant, events: Arc<RwLock<UserEventState>>) -> Self {
        Self {
            variant,
            user_events_state: events,
        }
    }
}

#[derive(Debug, Clone)]
pub enum PaneMessage {
    // Inner(pane_grid::)
    Clicked(pane_grid::Pane),
    Resized(pane_grid::ResizeEvent),
    TimeDisplaySettingChanged(TimeDisplaySetting),
    UserSelected(User),
    UserEvent(UserEvent),
}

impl From<UserEvent> for PaneMessage {
    fn from(v: UserEvent) -> Self {
        Self::UserEvent(v)
    }
}

type SharedState = Arc<RwLock<UserEventState>>;

pub struct PaneTab {
    shared_state: SharedState,
    time_display_setting: TimeDisplaySetting,
    panes: pane_grid::State<PaneState>,
    focus: Option<pane_grid::Pane>,
}

impl PaneTab {
    pub fn new() -> Self {
        let shared_state: SharedState = Default::default();

        let (mut panes, pane) =
            pane_grid::State::new(PaneState::new(PaneVariant::Users, shared_state.clone()));
        let (_, split) = panes
            .split(
                Axis::Vertical,
                &pane,
                PaneState::new(PaneVariant::UserEvents, shared_state.clone()),
            )
            .unwrap();
        panes.resize(&split, 0.25);

        Self {
            focus: None,
            panes,
            shared_state,
            time_display_setting: Default::default(),
        }
    }

    fn state_mut(&mut self) -> RwLockWriteGuard<'_, UserEventState> {
        self.shared_state
            .try_write()
            .expect("Should not write state while viewing")
    }

    pub fn update(&mut self, message: PaneMessage) -> Command<PaneMessage> {
        match message {
            PaneMessage::Clicked(pane) => {
                self.focus = Some(pane);
            }
            PaneMessage::Resized(pane_grid::ResizeEvent { split, ratio }) => {
                self.panes.resize(&split, ratio);
            }
            PaneMessage::UserSelected(user) => {
                let mut state = self.state_mut();
                state.selected_user = Some(user);
            }
            PaneMessage::UserEvent(user_event) => {
                let mut state = self.state_mut();

                let user = user_event.user;

                let id = state
                    .scroll_ids
                    .entry(user.clone())
                    .or_insert_with(|| scrollable::Id::unique())
                    .clone();

                state
                    .events
                    .entry(user)
                    .or_default()
                    .push((user_event.event, user_event.timestamp));

                return snap_to(id, 1.0);
            }
            PaneMessage::TimeDisplaySettingChanged(to) => {
                self.time_display_setting = to;
            }
        }

        Command::none()
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum TimeDisplaySetting {
    #[default]
    Absolute,
    Relative,
    None,
}

impl TimeDisplaySetting {
    const ALL: [TimeDisplaySetting; 3] = [
        TimeDisplaySetting::Absolute,
        TimeDisplaySetting::Relative,
        TimeDisplaySetting::None,
    ];
}

impl From<TimeDisplaySetting> for String {
    fn from(setting: TimeDisplaySetting) -> Self {
        match setting {
            TimeDisplaySetting::Absolute => "Absolute".to_string(),
            TimeDisplaySetting::Relative => "Relative".to_string(),
            TimeDisplaySetting::None => "None".to_string(),
        }
    }
}

impl Tab for PaneTab {
    type Message = Message;

    fn title(&self) -> String {
        String::from("User Events")
    }

    fn tab_icon(&self) -> crate::Icon {
        Icon::Heart
    }

    fn content(&self) -> Element<Message> {
        let pane_grid: Element<PaneMessage> =
            PaneGrid::new(&self.panes, |pane, state, _is_maximized| {
                let is_focused = self.focus == Some(pane);

                // let title = row![
                //     "Pane",
                //     text(pane.id.to_string()).style(if is_focused {
                //         PANE_ID_COLOR_FOCUSED
                //     } else {
                //         PANE_ID_COLOR_UNFOCUSED
                //     }),
                // ];

                // let title_bar = pane_grid::TitleBar::new(title).padding(5);

                pane_grid::Content::new(state.view_content(self.time_display_setting))
                    // .title_bar(title_bar)
                    .style(if is_focused {
                        style::pane_focused
                    } else {
                        style::pane_active
                    })
            })
            .spacing(5)
            .width(Length::Fill)
            .height(Length::Fill)
            .on_click(PaneMessage::Clicked)
            .on_resize(10, PaneMessage::Resized)
            .into();

        let time_settings: Element<PaneMessage> = TimeDisplaySetting::ALL
            .iter()
            .cloned()
            .fold(
                widget::column![text("Time display")].padding(5).spacing(5),
                |column, setting| {
                    column.push(
                        Radio::new(
                            setting,
                            setting,
                            Some(self.time_display_setting),
                            PaneMessage::TimeDisplaySettingChanged,
                        )
                        .size(10),
                    )
                },
            )
            .into();

        let time_settings: Element<PaneMessage> = container(time_settings)
            .width(Length::Fill)
            .height(Length::Shrink)
            .center_y()
            .into();

        let contents = widget::column![
            time_settings.map(Message::Pane),
            pane_grid.map(Message::Pane)
        ];

        container(contents)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }
}

// fn view_content<'a>(state: &PaneState) -> Element<'a, PaneMessage> {
//     dbg!(state);
//     let some_buttons = row![Button::new(text("Hey"))];

//     let users = column(
//         (0..100)
//             .into_iter()
//             .map(|i| Text::new(format!("User #{}", i)).into())
//             .collect(),
//     )
//     .width(Length::Fill);

//     let contents = column![some_buttons, users];

//     container(scrollable(contents))
//         .padding(5)
//         .width(Length::Fill)
//         .height(Length::Fill)
//         .into()
// }

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

mod style {
    use iced::widget::container;
    use iced::Theme;

    pub fn title_bar_active(theme: &Theme) -> container::Appearance {
        let palette = theme.extended_palette();

        container::Appearance {
            text_color: Some(palette.background.strong.text),
            background: Some(palette.background.strong.color.into()),
            ..Default::default()
        }
    }

    pub fn title_bar_focused(theme: &Theme) -> container::Appearance {
        let palette = theme.extended_palette();

        container::Appearance {
            text_color: Some(palette.primary.strong.text),
            background: Some(palette.primary.strong.color.into()),
            ..Default::default()
        }
    }

    pub fn pane_active(theme: &Theme) -> container::Appearance {
        let palette = theme.extended_palette();

        container::Appearance {
            background: Some(palette.background.weak.color.into()),
            border_width: 2.0,
            border_color: palette.background.strong.color,
            ..Default::default()
        }
    }

    pub fn pane_focused(theme: &Theme) -> container::Appearance {
        let palette = theme.extended_palette();

        container::Appearance {
            background: Some(palette.background.weak.color.into()),
            border_width: 2.0,
            border_color: palette.primary.strong.color,
            ..Default::default()
        }
    }
}
