// TODO: Testcases!
//
// Especially:
//
// 1. Available, hold it
// 2. Busy, get queued
// 3. Busy, also queed
// 4. First in queue drops
// 5. Semaphore dropped
// 6. Check that first in queue is ignored, second in place gets it
//
// Also check things like if there was a queue, but all queuers dropped, _then_ someone arrives.
// And so on.

use color_eyre::Result;
// use serial_keel::{
//     actions::{Action, Response},
//     endpoint::EndpointLabel,
// };
// use tracing::info;

// use common::{connect, receive, send_receive};

mod common;

#[tokio::test]
async fn second_user_is_queued() -> Result<()> {
    // TODO: Since mock endpoints are unique per user,
    // how do we test this?
    // Perhaps we could add an override if cfg test, somehow?

    Ok(())
}
