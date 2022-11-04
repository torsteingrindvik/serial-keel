import pytest
import logging
from pathlib import Path

from serial_keel import connect


# logging.basicConfig(level=logging.DEBUG)

# Note that for mocks to actually share resources,
# we need the `mocks-share-endpoints` feature to be enabled.
# E.g.: `cargo r --features mocks-share-endpoints`


@pytest.mark.asyncio_cooperative
@pytest.mark.parametrize("n", range(3))
async def test_crypto_test_app(n):
    logger = logging.getLogger(f'logger-{n}')
    h = logging.FileHandler(f'log-{n}.log', mode='w')
    h.setFormatter(logging.Formatter(
        '%(asctime)s [%(levelname)s] %(message)s'))
    h.setLevel(logging.DEBUG)
    logger.setLevel(logging.DEBUG)
    logger.addHandler(h)

    async with connect("ws://127.0.0.1:3000/ws", logger) as sk:
        endpoint = await sk.control_mock('mock-crypto-test-app')
        logger.info('Controlling mock')

        await sk.observe_mock('mock-crypto-test-app', Path('mock/crypto-test-app.txt'))
        logger.info('Observing and file contents written')

        async for message in sk.endpoint_messages(endpoint):
            if 'PROJECT EXECUTION SUCCESSFUL' in message:
                break
