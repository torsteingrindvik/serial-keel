use std::path::PathBuf;

use clap::{Parser, Subcommand};
use serde::Serialize;

use crate::{
    actions::{self, Action},
    config::Config,
    endpoint::{EndpointId, LabelledEndpointId},
    error,
};

/// The command line interface for serial keel.
#[derive(Parser)]
#[command(author, version, about)]
pub struct Cli {
    /// Path to a configuration file
    pub config: Option<PathBuf>,

    /// Subcommands
    #[command(subcommand)]
    pub command: Option<Commands>,
}

/// Commands available in the command line interface.
#[derive(Subcommand)]
pub enum Commands {
    /// Examples for user convenience.
    #[clap(subcommand)]
    Examples(Examples),
}

/// Helpful examples for users.
#[derive(Subcommand, Clone)]
pub enum Examples {
    /// Show an example of a configuration file's contents.
    Config,

    /// Show an example of a somewhat complete session between a client and a server.
    /// Aims to showcase most of the things a client and server can exchange between them.
    Session,

    /// Examples relating to user requests.
    #[clap(subcommand)]
    Request(Request),

    /// Examples relating to server-to-user responses.
    #[clap(subcommand)]
    Response(Response),
}

/// Examples of requests a user may send to the server.
#[derive(Subcommand, Clone)]
pub enum Request {
    /// Show an example JSON request of controlling a specific mock endpoint.
    ControlMock,

    /// Show an example JSON request of controlling a specific TTY endpoint.
    ControlTty,

    /// Show an example JSON request of controlling any endpoint matching the provided label(s).
    ControlAny,

    /// Show an example JSON request of observing a specific TTY endpoint.
    ObserveTty,

    /// Show an example JSON request of observing a specific mock endpoint.
    ObserveMock,

    /// Show an example JSON request of writing a message to an endpoint.
    WriteMessage,

    /// Show an example JSON request of writing bytes to an endpoint.
    WriteMessageBytes,

    /// Show an example JSON request of observing all events from all sources.
    ObserveEvents,
}

/// Examples of responses a user might see from a server.
#[derive(Subcommand, Clone)]
pub enum Response {
    /// Show an example JSON response of a confirmation that writing a message was ok.
    WriteOk,

    /// Show an example JSON response of a confirmation that the user is now observing all events.
    ObservingEvents,

    /// Show an example JSON response of a new message received.
    NewMessage,

    /// Show an example JSON response to successfully starting to observe an endpoint.
    Observing,

    /// Show an example JSON response to granted control.
    ControlGranted,

    /// Show an example JSON response to being queued for control.
    ControlQueued,
}

/// Handle subcommands.
pub fn handle_command(command: Commands) {
    fn print_config() {
        let c = Config::example();
        println!("{}", c.serialize_pretty());
    }

    fn print_session() {
        fn c(comment: &str) {
            println!("// {comment}");
        }
        fn req(req: impl Serialize) {
            println!("> {}", serde_json::to_string(&req).unwrap());
        }
        fn resp(resp: impl Serialize) {
            println!("< {}", serde_json::to_string(&resp).unwrap());
        }
        fn resp_ok(r: impl Serialize) {
            let r: Result<_, error::Error> = Ok(r);
            resp(r)
        }

        let lei_0 = LabelledEndpointId::new(&EndpointId::tty("/dev/ttyACM0"));
        let lei_1 = LabelledEndpointId::new_with_labels(
            &EndpointId::tty("/dev/ttyACM1"),
            &["fast", "secure"],
        );
        let lei_2 = LabelledEndpointId::new_with_labels(
            &EndpointId::tty("/dev/ttyACM2"),
            &["fast", "secure", "expensive"],
        );

        c("Example session. User requests are prepended with >, server responses are prepended with <");
        c("");
        c("The user wants exclusive access over an endpoint");
        req(Action::control_tty("/dev/ttyACM0"));
        c("The endpoint was not in use so the user gets access right away");
        resp_ok(actions::Response::control_granted(vec![lei_0.clone()]));
        c("");
        c("The user also wants access to any endpoint matching a few labels");
        req(Action::control_any(&["fast", "secure"]));
        c("Two endpoints matched, neither were available, therefore queued");
        resp_ok(actions::Response::control_queue(vec![lei_1, lei_2.clone()]));
        c("The user sits around and waits for another response");
        c("Some time passes.. Then one is available");
        resp_ok(actions::Response::control_granted(vec![lei_2]));
        c("");
        c("The user wants to know about messages received too, so they observe an endpoint");
        req(Action::Observe(lei_0.clone().into()));
        resp_ok(actions::Response::observing(lei_0.clone()));
        c("");
        c("Messages might now appear at any time on that endpoint");
        c("The server does not assume an encoding for the message, so the user should decode it if it's e.g. utf-8");
        resp_ok(actions::Response::message(
            lei_0.clone(),
            "Hello, world".into(),
        ));
        c("");
        c("Since the user controls a few endpoints, the user may write to those at any time");
        req(Action::Write((lei_0.into(), "Hi there, endpoint!".into())));
        resp_ok(actions::Response::write_ok());
        c("");
        c("The user leaves and the endpoints they controlled are then available for others");
    }

    fn print_request(req: impl Serialize) {
        println!("{}", serde_json::to_string_pretty(&req).unwrap());
    }

    fn print_ok_response(req: impl Serialize) {
        let req: Result<_, error::Error> = Ok(req);
        println!("{}", serde_json::to_string_pretty(&req).unwrap());
    }

    // use cli::Examples;
    use Request::*;
    use Response::*;

    match command {
        Commands::Examples(example) => match example {
            Examples::Config => print_config(),
            Examples::Session => print_session(),
            Examples::Request(ControlMock) => {
                print_request(Action::example_control_mock());
            }
            Examples::Request(ControlTty) => {
                print_request(Action::example_control_tty());
            }
            Examples::Request(ControlAny) => {
                print_request(Action::example_control_any());
            }
            Examples::Request(ObserveTty) => {
                print_request(Action::example_observe_tty());
            }
            Examples::Request(ObserveMock) => {
                print_request(Action::example_observe_mock());
            }
            Examples::Request(WriteMessage) => {
                print_request(Action::example_write());
            }
            Examples::Request(WriteMessageBytes) => {
                print_request(Action::example_write_bytes());
            }
            Examples::Request(ObserveEvents) => {
                print_request(Action::example_observe_events());
            }
            Examples::Response(WriteOk) => {
                print_ok_response(actions::Response::example_write_ok());
            }
            Examples::Response(ObservingEvents) => {
                print_ok_response(actions::Response::example_observing_events());
            }
            Examples::Response(NewMessage) => {
                print_ok_response(actions::Response::example_new_message());
            }
            Examples::Response(Observing) => {
                print_ok_response(actions::Response::example_observing());
            }
            Examples::Response(ControlGranted) => {
                print_ok_response(actions::Response::example_control_granted());
            }
            Examples::Response(ControlQueued) => {
                print_ok_response(actions::Response::example_control_queue());
            }
        },
    }
}
