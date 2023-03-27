# Serial Keel Client for Python 3

For more information on the serial-keel project see https://github.com/torsteingrindvik/serial-keel/

## General structure

```python
from serialkeel import connect

logger = logging.getLogger(f'my-logger')
timeout = 100

# First we setup a websocket connection to an available Serial Keel server.
async with connect("ws://127.0.0.1:3123/client", timeout, logger) as sk:
    # We are interested only in endpoints which have both of these labels.
    labels = ["label-1", "label-2"]

    # Wait here until such an endpoint (or endpoints) are available.
    endpoints = await sk.control_any(labels)

    # We might have gained control over a group of endpoints.
    # Anyway we know that all endpoints we control have all required labels,
    # so just use the first one.
    endpoint = endpoints[0]

    # Tell the server we want to observe any messages received on the endpoint.
    await sk.observe(endpoint)

    # We control the endpoint, so we are allowed to write to it.
    await sk.write(endpoint, 'You can start now')

    async for message in sk.endpoint_messages(endpoint):
        logger.info(f'Message on {endpoint}: {message}')

        if 'Done' in message:
            break

```

## Example 

[This other example](https://github.com/torsteingrindvik/serial-keel/blob/main/py/examples/) shows 10 concurrent clients
accessing a Serial Keel server with mock endpoints. It uses pytest to run all clients concurrently.


### Server setup

So if not already done, install (from this folder):

`cargo install --path core`

Then run the server with the mock configuration:

`serial-keel py/examples/test-mock-concurrent.ron`

The above mock configuration file uses mock endpoints for the example.

### Python setup

```
pip install serialkeel
```

#### Pytest via command line

With the server running, do:

```text
  pytest ./py/examples/
```
