# Serial Keel

## Quickstart

- Start the server:
    - `cargo r`, or `cargo r --release`, or, `cargo r --features mocks-share-endpoints --release` (see [cargo features](#cargo-features))
	- Precompiled: `./bin/serial-keel`

## How it works

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

### Example: Client TTY session

```text
WIP

┌────────┐                ┌────────┐      ┌────────────┐
│ client │                │ server │      │/dev/ttyACM0│
└───┬────┘                └────┬───┘      └─────┬──────┘
    │                          │                │
    │                          │                │
    │                          │message("LOREM")│
    │                          │◄───────────────┤
    │                          │                │
    │                          │                │
    │                          │                │
    │                          │                │
    │  observe("/dev/ttyACM0") │                │
    ├─────────────────────────►│                │
    │                          │                │
    │                          │                │
    │                          │                │
    │                          │                │
    │                          │                │
    │                          │                │
    │                          │                │
    │                          │                │
    │                          │                │
    │                          │                │
    │                          │                │
    │                          │                │
    │                          │                │
    │                          │                │
    │                          │                │
    │                          │                │
    │                          │                │
    │                          │                │
    │                          │                │
    │       message("LOREM")   │                │
    │       from "mock-foo"    │                │
    │◄─────────────────────────┤                │
    │                          │                │
    │       message("IPSUM")   │                │
    │       from "mock-foo"    │                │
    │◄─────────────────────────┤                │
    │                          │                │
    │       message("FOO")     │                │
    │       from "mock-foo"    │                │
    │◄─────────────────────────┤                │
    │                          │                │
    │                          │                │
   ─┴─                         │                │
client                         │                │
disconnects                    │                │
                       remove  │                │
                     "mock-foo"│                │
                               │                │
```

## Cargo Features

### `mocks-share-endpoints`

If a client connects and asks for control of `mock-foo`, then this endpoint is created on the spot.
This is to support mocking and not needing a separate API just to create mock endpoints.

