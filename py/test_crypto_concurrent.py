import pytest
import logging
from pathlib import Path

from serial_keel import connect

logging.basicConfig(format="%(asctime)s     %(message)s", level=logging.INFO)

# Note that for mocks to actually share resources,
# we need the `mocks-share-endpoints` feature to be enabled.
# E.g.: `cargo r --features mocks-share-endpoints`


@pytest.mark.asyncio_cooperative
@pytest.mark.parametrize("n", range(10))
async def test_crypto_test_app(n):
    logging.info("I am {n}")

    async with connect("ws://127.0.0.1:3000/ws") as sk:
        endpoint = await sk.observe_mock('mock-crypto-test-app', Path('mock/crypto-test-app.txt'))
        async for message in sk.endpoint_messages(endpoint):
            if 'PROJECT EXECUTION SUCCESSFUL' in message:
                break
