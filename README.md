# Serial Keel

A server which helps ease working with TTYs.

Features:

- Server continuously monitors TTYs
  - No messages lost!
- An endpoint (e.g. `/dev/ttyACM0`) can have any number of listeners (called _observers_)
  - This means even though an endpoint is in use, others may observe what's going on
- Clients can `await` exclusive control over an endpoint
  - This provides write access
- Endpoints can be put into logical groups
  - Exclusive access now implies exclusivity over the whole group
- Any number of clients can wait for write access- the server automatically queues them (FIFO)
- Endpoints can be mocked
  - Write to it to instruct it to send back messages the same way a real device would
  - Allows separating TTY message logic from actual devices for rapid prototyping

## Quickstart

If you have installed serial keel (see [the install step](#running-a-server) below), please use `serial-keel help` to explore what's available.

### Running a server
Start the server. Choose one of:

1. `cargo r`, or `cargo r --release`, or, `cargo r --features mocks-share-endpoints --release` (see [cargo features](#cargo-features))
3. Install, e.g.: `cargo install --path serial-keel --features mocks-share-endpoints`, then run it: `serial-keel`
2. Precompiled: `./bin/serial-keel` (TODO: Update this bin)

Use the environment variable `RUST_LOG` to control log verbosity, e.g. `export RUST_LOG=warn` or `export RUST_LOG=debug`.

### Using a configuration file

Run `serial-keel examples config` to see an example configuration file.
You can store this as `my-config.ron` to get started.

## How it works

### Concepts

#### Client

The actor initiating a websocket connection to some active server.
Sends requests to the server, which replies with responses.

#### Server

Serves clients over websockets.
Continuously listens to TTYs.

#### Endpoint

A thing which may produce messages and accepts being written to.

For example the endpoint `/dev/ttyACM0` or `COM0` represents real TTYs which can produce messages.

Endpoints may also be mocked, and thus have any name e.g. `mock-1` or `specific-mocked-device-30`.

### Example: Client TTY session

Shows a client observing a single TTY endpoint.
The concept works the same for more endpoints.

- The TTY may at any point produce a message to the server
- The server will forward messages to any observer(s)
- Clients starting to observe will receive messages _from that point on_.

```text
┌────────┐                ┌────────┐      ┌────────────┐
│ client │                │ server │      │/dev/ttyACM0│
└───┬────┘                └────┬───┘      └─────┬──────┘
    │                          │                │
    │                          │                │
    │                          │message("LOREM")│
    │              no observers│◄───────────────┤
    │                    x─────┤                │
    │                          │                │
    │                          │                │
    │  observe("/dev/ttyACM0") │                │
    ├─────────────────────────►│                │
    │                          │                │
    │                          │                │
    │                          │                │
    │   message("LOREM")       │    "LOREM"     │
    │   from "/dev/ttyACM0"    │◄───────────────┤
    │◄─────────────────────────┤                │
    │                          │                │
    │   message("IPSUM")       │    "IPSUM"     │
    │   from "/dev/ttyACM0"    │◄───────────────┤
    │◄─────────────────────────┤                │
    │                          │                │
    │   message("HELLO")       │    "HELLO"     │
    │   from "/dev/ttyACM0"    │◄───────────────┤
    │◄─────────────────────────┤                │
    │                          │                │
    │                          │                │
   ─┴─                         │                │
client                         │                │
disconnects                    │                │
```

### Example: Client mock session

This example shows the message passing
between a client and server for a mock session.

When a user asks to control a mock, the mock is created on the spot.
The mock endpoint (here `mock-foo`) is also unique for this user.

The mock cannot know what to emulate without being instructed on what to do.
Therefore write commands to a mock is echoed back, but split by lines.

This allows writing a whole text file which is then sent back line by line.

Note that requests from the client are always responded to right away,
but messages on an endpoint are sent asynchronously to the client.

When the user disconnects the mock is removed leaving no state.

```text
┌────────┐                ┌────────┐
│ client │                │ server │
└───┬────┘                └────┬───┘
    │                          │
    │                          │
    │     control("mock-foo")  │
    ├─────────────────────────►│
    │                          ├───────────┐
    │                          │ initialize│
    │                          │"mock-foo" │
    │     control granted      │◄──────────┘
    │◄─────────────────────────┤
    │                          │
    │                          │
    │     observe("mock-foo")  │
    ├─────────────────────────►│
    │            ok            │
    │◄─────────────────────────┤ 
    │                          │
    │                          │
    │                          │
    │write("LOREM\nIPSUM\nFOO")│
    │  to endpoint "mock-foo"  │
    │                          │
    ├─────────────────────────►│
    │                          ├───────────┐
    │                          │ "mock-foo"│
    │                          │ receives  │
    │                          │ text      │
    │        write ok          │◄──────────┘
    │◄─────────────────────────┤
    │                          │
    │       message("LOREM")   │
    │       from "mock-foo"    │
    │◄─────────────────────────┤
    │                          │
    │       message("IPSUM")   │
    │       from "mock-foo"    │
    │◄─────────────────────────┤
    │                          │
    │       message("FOO")     │
    │       from "mock-foo"    │
    │◄─────────────────────────┤
    │                          │
    │                          │
   ─┴─                         │
client                         │
disconnects                    │
                       remove  │
                     "mock-foo"│
                               │
```

### Example: Labelled group control

This example shows a more involved example.

- Three clients
- Four endpoints
- Endpoints are grouped into two groups
- The groups share an arbitrary label "device-combo"

The concept shown here is that clients may ask to control any endpoint (or group) which matches some label.
Labels are set via the server configuration file (see [using a configuration file](#using-a-configuration-file)).

The other concept shown is that control is tied to the connection of the client.
When a client disconnects control is released and the next in queue (if any) gets control.

See below the diagram for an explanation of the numbered events in this example.

```text
┌─────────┐ ┌─────────┐ ┌─────────┐             ┌────────┐ ┌────────────────────────────────┐ ┌────────────────────────────────┐
│ client1 │ │ client2 │ │ client3 │             │ server │ │ group1, label: "device-combo"  │ │ group2, label: "device-combo"  │
└───┬─────┘ └───┬─────┘ └───┬─────┘             └────┬───┘ │                                │ │                                │
    │           │           │ control-any(           │     │ ┌────────────┐  ┌────────────┐ │ │ ┌────────────┐  ┌────────────┐ │
    │ 1.        │           │   "device-combo")      │     │ │/dev/ttyACM0│  │/dev/ttyACM1│ │ │ │/dev/ttyACM3│  │/dev/ttyACM4│ │
    ├───────────┼───────────┼───────────────────────►│     │ └─────┬──────┘  └─────┬──────┘ │ │ └─────┬──────┘  └─────┬──────┘ │
    │           │           │  control granted       │     │       │               │        │ │       │               │        │
    │ 2.        │           │  (group1)              │     └───────┼───────────────┼────────┘ └───────┼───────────────┼────────┘
    │◄──────────┼───────────┼────────────────────────┤             │               │                  │               │
    │           │           │                        │             │               │                  │               │
    │           │           │                        │             │               │                  │               │
    │           │           │                        │             │               │                  │               │
    │           │           │                        │             │               │                  │               │
    │           │           │                        │             │               │                  │               │
    │           │           │  control-any(          │             │               │                  │               │
    │           │ 3.        │    "device-combo")     │             │               │                  │               │
    │           ├───────────┼───────────────────────►│             │               │                  │               │
    │           │           │  control granted       │             │               │                  │               │
    │           │ 4.        │  (group2)              │             │               │                  │               │
    │           │◄──────────┼────────────────────────┤             │               │                  │               │
    │           │           │                        │             │               │                  │               │
    │           │           │                        │             │               │                  │               │
    │           │           │                        │             │               │                  │               │
    │           │           │                        │             │               │                  │               │
    │           │           │  control-any(          │             │               │                  │               │
    │           │           │    "device-combo")     │             │               │                  │               │
    │           │        5. ├───────────────────────►│             │               │                  │               │
    │           │           │                        │             │               │                  │               │
    │           │           │  queued                │             │               │                  │               │
    │           │        6. │◄───────────────────────┤             │               │                  │               │
    │           │           │                        │             │               │                  │               │
    │           │           │                        │             │               │                  │               │
    │           │           │                        │             │               │                  │               │
    │           │           │                        │             │               │                  │               │
    │           │           │  write("hello world")  │             │               │                  │               │
    │           │ 7.        │  to "/dev/ttyACM3"     │  "hello     │               │                  │               │
    │           ├───────────┼───────────────────────►│   world"    │               │                  │               │
    │           │           │                        ├─────────────┼───────────────┼─────────────────►│               │
    │           │ 8.        │  write ok              │             │               │                  │               │
    │           │◄──────────┼────────────────────────┤             │               │                  │               │
    │           │           │                        │             │               │                  │               │
    │           │           │                        │             │               │                  │               │
    │       9. ─┴─          │                        │             │               │                  │               │
    │        client         │                        │             │               │                  │               │
    │        disconnects    │                        │             │               │                  │               │
    │        (group2 now    │                        │             │               │                  │               │
    │         available)    │                        │             │               │                  │               │
    │                       │                        │             │               │                  │               │
    │                       │                        │             │               │                  │               │
    │                       │  control granted       │             │               │                  │               │
    │                       │  (group2)              │             │               │                  │               │
    │                   10. │◄───────────────────────┤             │               │                  │               │
    │                       │                        │             │               │                  │               │
    │                       │                        │             │               │                  │               │
 11.│                   12. │                        │             │               │                  │               │
   ─┴─                     ─┴─                       │             │               │                  │               │
client                  client                       │             │               │                  │               │
disconnects             disconnects
(group1 now             (group2 now
 available)              available)
```

Explanation of events:

1. `client1` asks to control anything matching the label "device-combo".
2. The server grants control to all endpoints in `group1` since it matched the label and was available. Note that the server might as well have given access to `group2` here.
3. `client2` asks control over any "device-combo" too.
4. `group2` matched and was available and is granted.
5. `client3` asks control over any "device-combo".
6. The server sees two groups matching, but all are taken. It queues the client.
7. `client2` has control over endpoints in `group2`. The server sent information about those endpoints (not shown), but `client2` therefore knows `/dev/ttyACM3` is controllable. `client2` writes a message to `/dev/ttyACM3`.
8. The server saw that `client2` had write access to `/dev/ttyACM3`. It wrote the message to the endpoint, i.e. it put the message "on wire". It therefore sends a "write ok" message back to the client.
9. `client2` leaves. The server notices and frees the resources `client2` had, which means `group2` is now available again.
10. The server saw that a resource matching what `client3` wants is now available, and grants `client3` control over it.
11. `client1` leaves (without ever using its resources just to make the example simpler). This frees `group1`.
12. `client3` leaves. This frees `group2`.

## Cargo Features

### `mocks-share-endpoints`

If a client connects and asks for control of `mock-foo`, then this endpoint is created on the spot.
This is to support mocking and not needing a separate API just to create mock endpoints.

