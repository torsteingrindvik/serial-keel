use chrono::{DateTime, Utc};
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use enum_iterator::Sequence;
use serial_keel::{
    client::{UserEvent, UserEventReader},
    endpoint::{EndpointId, InternalEndpointInfo},
    user::User,
};
use std::{
    collections::{BTreeSet, HashMap},
    error::Error,
    // fmt::Display,
    io,
    time::{Duration, Instant},
};
use tui::{
    backend::{Backend, CrosstermBackend},
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Span, Spans},
    widgets::*,
    Frame, Terminal,
};

// struct StatefulList<T> {
//     state: ListState,
//     items: Vec<T>,
// }

// impl<T> StatefulList<T> {
//     fn with_items(items: Vec<T>) -> StatefulList<T> {
//         let mut state = ListState::default();
//         state.select(Some(0));
//         StatefulList { state, items }
//     }

//     fn next(&mut self) {
//         let i = match self.state.selected() {
//             Some(i) => {
//                 if i >= self.items.len() - 1 {
//                     0
//                 } else {
//                     i + 1
//                 }
//             }
//             None => 0,
//         };
//         self.state.select(Some(i));
//     }

//     fn previous(&mut self) {
//         let i = match self.state.selected() {
//             Some(i) => {
//                 if i == 0 {
//                     self.items.len() - 1
//                 } else {
//                     i - 1
//                 }
//             }
//             None => 0,
//         };
//         self.state.select(Some(i));
//     }

//     fn unselect(&mut self) {
//         self.state.select(None);
//     }
// }

#[derive(Debug, Copy, Clone, PartialEq, Sequence)]
enum Tab {
    Serial,
    Users,
    Server,
}

impl Tab {
    fn index(&self) -> usize {
        match self {
            Tab::Serial => 0,
            Tab::Users => 1,
            Tab::Server => 2,
        }
    }

    // fn next(&mut self) {
    //     *self = match self {
    //         Tab::Serial => Tab::Users,
    //         Tab::Users => Tab::Server,
    //         Tab::Server => Tab::Serial,
    //     }
    // }

    // fn previous(&mut self) {
    //     *self = match self {
    //         Tab::Serial => Tab::Server,
    //         Tab::Users => Tab::Serial,
    //         Tab::Server => Tab::Users,
    //     }
    // }
}

// impl Display for Tab {
//     fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
//         match self {
//             Tab::Serial => write!(f, "Serial"),
//             Tab::Users => write!(f, "Users"),
//             Tab::Server => write!(f, "Server"),
//         }
//     }
// }

fn ui_example<B: Backend>(f: &mut Frame<B>, area: Rect, color: Color) {
    let w = Sparkline::default()
        .block(Block::default().title("Sparkline").borders(Borders::ALL))
        .data(&[
            0, 2, 3, 4, 1, 4, 10, 0, 2, 3, 4, 1, 4, 10, 0, 2, 3, 4, 1, 4, 10, 0, 2, 3, 4, 1, 4, 10,
        ])
        .max(5)
        .style(Style::default().fg(color).bg(Color::White));

    f.render_widget(w, area);
}

fn extend_set(set: &mut BTreeSet<EndpointId>, with: Vec<InternalEndpointInfo>) {
    set.extend(with.into_iter().map(|e| e.id.into()));
}

fn shrink_then_move(
    active: &mut BTreeSet<EndpointId>,
    inactive: &mut BTreeSet<EndpointId>,
    no_longer_active: Vec<InternalEndpointInfo>,
) {
    extend_set(inactive, no_longer_active);
    *active = active.difference(inactive).cloned().collect();
}

fn set_to_list_item(set: &BTreeSet<EndpointId>, active: bool) -> Vec<ListItem> {
    let style = if active {
        Style::default().fg(Color::Green)
    } else {
        Style::default().fg(Color::Red)
    };

    set.iter()
        .map(|e| ListItem::new(format!("{}", e)).style(style))
        .collect()
}

fn info_list<'i>(
    name: &'static str,
    active: &'i BTreeSet<EndpointId>,
    inactive: &'i BTreeSet<EndpointId>,
) -> impl Widget + 'i {
    let mut list_items = set_to_list_item(active, true);
    list_items.extend(set_to_list_item(inactive, false));

    List::new(list_items).block(Block::default().title(name).borders(Borders::ALL))
}

#[derive(Default)]
struct ActiveInactive<T> {
    inner: T,
    active: Option<DateTime<Utc>>,
    inactive: Option<DateTime<Utc>>,
}

impl<T> ActiveInactive<T> {
    // fn new(inner: T) -> Self {
    //     Self {
    //         inner,
    //         active: None,
    //         inactive: None,
    //     }
    // }

    fn is_active(&self) -> bool {
        self.active.is_some() && self.inactive.is_none()
    }

    fn set_active(&mut self, timestamp: DateTime<Utc>) {
        self.active = Some(timestamp);
        // Such that we don't have negative durations
        self.inactive = None;
    }

    fn set_inactive(&mut self, timestamp: DateTime<Utc>) {
        self.inactive = Some(timestamp);
    }

    fn seconds_alive(&self) -> Option<i64> {
        if let Some(active) = self.active {
            self.inactive
                .map(|inactive| (inactive - active).num_seconds())
        } else {
            None
        }
    }
}

#[derive(Default)]
struct UserState {
    events: Vec<serial_keel::client::Event>,

    connected: ActiveInactive<()>,
    // connected: Option<chrono::DateTime<Utc>>,
    // disconnected: Option<chrono::DateTime<Utc>>,
    observing: BTreeSet<EndpointId>,
    controlling: BTreeSet<EndpointId>,
    queued_for: BTreeSet<EndpointId>,
    no_longer_observing: BTreeSet<EndpointId>,
    no_longer_controlling: BTreeSet<EndpointId>,
    no_longer_queued_for: BTreeSet<EndpointId>,
}

impl UserState {
    fn add_event(&mut self, event: serial_keel::client::Event, timestamp: chrono::DateTime<Utc>) {
        use serial_keel::client::Event;

        self.events.push(event.clone());

        match event {
            Event::Connected => self.connected.set_active(timestamp),
            Event::Disconnected => self.connected.set_inactive(timestamp),

            Event::Observing(endpoints) => extend_set(&mut self.observing, endpoints),

            Event::NoLongerObserving(endpoints) => shrink_then_move(
                &mut self.observing,
                &mut self.no_longer_observing,
                endpoints,
            ),
            Event::InQueueFor(endpoints) => extend_set(&mut self.queued_for, endpoints),
            Event::InControlOf(endpoints) => extend_set(&mut self.controlling, endpoints),
            Event::NoLongerInQueueOf(endpoints) => shrink_then_move(
                &mut self.queued_for,
                &mut self.no_longer_queued_for,
                endpoints,
            ),
            Event::NoLongerInControlOf(endpoints) => shrink_then_move(
                &mut self.controlling,
                &mut self.no_longer_controlling,
                endpoints,
            ),
        }
    }

    fn user_display_name(&self, user: &User) -> ListItem {
        if let Some(seconds_alive) = self.connected.seconds_alive() {
            if self.connected.is_active() {
                ListItem::new(format!("[{:4}s] {user}", seconds_alive))
                    .style(Style::default().fg(Color::Green))
            } else {
                ListItem::new(format!("[{:4}s] {user}", seconds_alive))
                    .style(Style::default().fg(Color::Red))
            }
        } else {
            ListItem::new(user.to_string())
        }

        // match (self.connected, self.disconnected) {
        //     (None, None) => ListItem::new(user.to_string()),
        //     (None, Some(_)) => ListItem::new(user.to_string()),
        //     (Some(connected), None) => {
        //         let time_alive = (Utc::now() - connected).num_seconds();
        //     }
        //     (Some(connected), Some(disconnected)) => {
        //         let time_alive = (disconnected - connected).num_seconds();
        //         ListItem::new(format!("[{:4}s] {user}", time_alive))
        //             .style(Style::default().fg(Color::Red))
        //     }
        // }
    }

    fn observing_widget(&self) -> impl Widget + '_ {
        info_list("Observing", &self.observing, &self.no_longer_observing)
    }

    fn controlling_widget(&self) -> impl Widget + '_ {
        info_list(
            "Controlling",
            &self.controlling,
            &self.no_longer_controlling,
        )
    }
    fn queued_widget(&self) -> impl Widget + '_ {
        info_list("Queued", &self.queued_for, &self.no_longer_queued_for)
    }

    fn messages_widget(&self) -> impl Widget {
        let items = [
            ListItem::new("Messages 1"),
            ListItem::new("Item 2"),
            ListItem::new("Item 3"),
        ];
        List::new(items)
            .block(Block::default().title("Messages").borders(Borders::ALL))
            .style(Style::default().fg(Color::White))
    }

    fn render<B: Backend>(&self, f: &mut Frame<B>, area: Rect) {
        let top_bottom = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(10), Constraint::Min(0)].as_ref())
            .split(area);
        let top = top_bottom[0];
        let bottom = top_bottom[1];

        let left_middle_right = Layout::default()
            .direction(Direction::Horizontal)
            .constraints(
                [
                    Constraint::Percentage(33),
                    Constraint::Percentage(33),
                    Constraint::Min(0),
                ]
                .as_ref(),
            )
            .split(top);

        let top_left = left_middle_right[0];
        let top_middle = left_middle_right[1];
        let top_right = left_middle_right[2];

        f.render_widget(self.observing_widget(), top_left);
        f.render_widget(self.controlling_widget(), top_middle);
        f.render_widget(self.queued_widget(), top_right);

        f.render_widget(self.messages_widget(), bottom);
    }
}

#[derive(Default)]
struct Users {
    // Users and their data
    inner: HashMap<User, UserState>,

    ui_state: ListState,
}

impl Users {
    fn add_user_event(&mut self, user_event: UserEvent) {
        self.inner
            .entry(user_event.user)
            .or_default()
            .add_event(user_event.event, user_event.timestamp);
    }

    fn users(&self) -> Vec<&User> {
        self.inner.keys().collect()
    }

    fn next(&mut self) {
        let users = self.users();

        let i = match self.ui_state.selected() {
            Some(i) => {
                if i == self.users().len() - 1 {
                    Some(0)
                } else {
                    Some(i + 1)
                }
            }
            None => {
                if users.is_empty() {
                    None
                } else {
                    Some(0)
                }
            }
        };
        self.ui_state.select(i);
    }

    fn previous(&mut self) {
        let users = self.users();

        let i = match self.ui_state.selected() {
            Some(i) => {
                if i == 0 {
                    Some(users.len() - 1)
                } else {
                    Some(i - 1)
                }
            }
            None => {
                if users.is_empty() {
                    None
                } else {
                    Some(users.len() - 1)
                }
            }
        };
        self.ui_state.select(i);
    }

    fn render<B: Backend>(&self, f: &mut Frame<B>, area: Rect) {
        let left_right = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(15), Constraint::Min(0)].as_ref())
            .split(area);
        let left = left_right[0];
        let right = left_right[1];
        // let connected = |user| { if self.inner.get(user).unwrap().connected { "🟢" } else { "🔴" } };

        let users = self
            .inner
            .iter()
            .map(|(user, state)| {
                state.user_display_name(user)
                // ListItem::new(u.to_string()).style({
                //     let mut style = Style::default();
                //     if self.inner.get(u).unwrap().connected {
                //         style = style.add_modifier(Modifier::BOLD);
                //     }
                //     style
                // })
            })
            .collect::<Vec<_>>();

        f.render_widget(
            List::new(users)
                .block(Block::default().borders(Borders::all()).title("Users [↑↓]"))
                // TODO
                .highlight_style(Style::default().bg(Color::Green)),
            left,
        );

        if let Some(selected) = self.ui_state.selected() {
            let user = self.users()[selected];
            let user_state = self.inner.get(user).unwrap();

            user_state.render(f, right);
        } else {
            // f.render_widget(todo!(), right);
        }

        // ui_example(f, area, Color::Blue)
    }
}

/// This struct holds the current state of the app. In particular, it has the `items` field which is a wrapper
/// around `ListState`. Keeping track of the items state let us render the associated widget with its state
/// and have access to features such as natural scrolling.
///
/// Check the event handling at the bottom to see how to change the state on incoming events.
/// Check the drawing logic for items on how to specify the highlighting style for selected items.
struct App {
    tab: Tab,
    raw_events: Vec<UserEvent>,
    users: Users,
}

impl App {
    fn new() -> App {
        App {
            raw_events: vec![],
            tab: Tab::Serial,
            users: Users::default(),
        }
    }

    fn up(&mut self) {
        match self.tab {
            Tab::Serial | Tab::Server => {}
            Tab::Users => {
                self.users.next();
            }
        }
    }

    fn down(&mut self) {
        match self.tab {
            Tab::Serial | Tab::Server => {}
            Tab::Users => {
                self.users.previous();
            }
        }
    }

    fn add_user_event(&mut self, event: UserEvent) {
        self.raw_events.push(event.clone());
        self.users.add_user_event(event);
    }

    fn next_tab(&mut self) {
        self.tab = enum_iterator::next_cycle(&self.tab).unwrap();
    }

    fn previous_tab(&mut self) {
        self.tab = enum_iterator::previous_cycle(&self.tab).unwrap();
    }

    fn tabs(&self) -> impl Iterator<Item = Tab> {
        enum_iterator::all::<Tab>()
    }

    fn tab_index(&self) -> usize {
        self.tab.index()
    }

    fn ui_serial<B: Backend>(&self, f: &mut Frame<B>, area: Rect) {
        ui_example(f, area, Color::Blue)
    }
    fn ui_users<B: Backend>(&self, f: &mut Frame<B>, area: Rect) {
        // TODO: Left side is a list of users, right side is the user state for the selected user.
        // ui_example(f, area, Color::Red)
        self.users.render(f, area);
    }
    fn ui_server<B: Backend>(&self, f: &mut Frame<B>, area: Rect) {
        ui_example(f, area, Color::Green)
    }

    fn tab_widget<B: Backend>(&self, f: &mut Frame<B>, area: Rect) {
        match self.tab {
            Tab::Serial => self.ui_serial(f, area),
            Tab::Users => self.ui_users(f, area),
            Tab::Server => self.ui_server(f, area),
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    // connect to server
    let mut sk_client = serial_keel::client::ClientHandle::new("localhost", 3123).await?;
    let user_events = sk_client.observe_user_events().await?;

    // setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // create app and run it
    let tick_rate = Duration::from_millis(50);
    let app = App::new();
    let res = run_app(&mut terminal, app, user_events, tick_rate);

    // restore terminal
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    if let Err(err) = res {
        println!("{:?}", err)
    }

    Ok(())
}

fn run_app<B: Backend>(
    terminal: &mut Terminal<B>,
    mut app: App,
    mut user_events: UserEventReader,
    tick_rate: Duration,
) -> io::Result<()> {
    let mut last_tick = Instant::now();
    loop {
        terminal.draw(|f| ui(f, &mut app))?;

        let timeout = tick_rate.saturating_sub(last_tick.elapsed());

        if event::poll(timeout)? {
            if let event::Event::Key(key) = event::read()? {
                match key.code {
                    KeyCode::Char('q') => return Ok(()),
                    KeyCode::Left | KeyCode::Char('h') => app.previous_tab(),
                    KeyCode::Right | KeyCode::Char('l') => app.next_tab(),
                    KeyCode::Down => app.up(),
                    KeyCode::Up => app.down(),
                    _ => {}
                }
            }
        }
        if last_tick.elapsed() >= tick_rate {
            last_tick = Instant::now();
        }

        while let Some(user_event) = user_events.try_next_user_event() {
            app.add_user_event(user_event);
        }
    }
}

// fn widget_serial() {}

fn ui<B: Backend>(f: &mut Frame<B>, app: &mut App) {
    // Create two chunks with equal horizontal screen space
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(0)].as_ref())
        .split(f.size());

    let titles = app
        .tabs()
        .map(|t| {
            Spans::from(vec![Span::styled(
                format!("{:?}", t),
                Style::default().fg(Color::LightGreen),
            )])
        })
        .collect();

    let tabs = Tabs::new(titles)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Serial Keel TUI"),
        )
        .select(app.tab_index())
        .style(Style::default().fg(Color::White))
        .highlight_style(Style::default().add_modifier(Modifier::BOLD | Modifier::UNDERLINED));

    f.render_widget(tabs, chunks[0]);

    // let inner = match app.tabs {

    // }

    // let user_events: Vec<ListItem> = app
    //     .user_events
    //     .iter()
    //     .rev()
    //     .map(|user_event| ListItem::new(user_event.to_string()))
    //     .collect();

    // let events_list = List::new(user_events)
    //     .block(Block::default().borders(Borders::ALL).title("List"))
    //     .start_corner(Corner::BottomLeft);
    // let sub_areas = Layout::default()
    //     .direction(Direction::Horizontal)
    //     .constraints([Constraint::Percentage(50), Constraint::Percentage(50)].as_ref())
    //     .split(chunks[1]);

    app.tab_widget(f, chunks[1]);
    // f.render_widget(widget, sub_areas[0]);

    // let widget = app.tab_widget();
    // f.render_widget(widget, sub_areas[1]);
}
