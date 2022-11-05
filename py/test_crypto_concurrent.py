import pytest
import logging
from pathlib import Path

from serial_keel import connect, Endpoint, EndpointType


# logging.basicConfig(level=logging.DEBUG)

# Note that for mocks to actually share resources,
# we need the `mocks-share-endpoints` feature to be enabled.
# E.g.: `cargo r --features mocks-share-endpoints`


@pytest.mark.asyncio_cooperative
@pytest.mark.parametrize("n", range(1))
async def test_crypto_test_app(n):
    logger = logging.getLogger(f'logger-{n}')
    h = logging.FileHandler(f'logs/log-{n}.log', mode='w')
    h.setFormatter(logging.Formatter(
        '%(asctime)s [%(levelname)s] %(message)s'))
    h.setLevel(logging.DEBUG)
    logger.setLevel(logging.DEBUG)

    # Note that depending on how many tests parameterized,
    # this almost doubles time executed
    logger.addHandler(h)  # <--

    mock = False  # TODO: Pass via cli

    async with connect("ws://127.0.0.1:3123/ws", logger) as sk:
        if mock:
            endpoint = Endpoint('mock-crypto-test-app', EndpointType.MOCK)
        else:
            endpoint = Endpoint('COM22', EndpointType.TTY)

        await sk.control(endpoint)
        logger.info('Controlling endpoint')

        await sk.observe(endpoint)
        logger.info('Observing endpoint')

        if mock:
            await sk.write_file(endpoint, Path('mock/crypto-test-app.txt'))

        async for message in sk.endpoint_messages(endpoint):
            if 'PROJECT EXECUTION SUCCESSFUL' in message:
                break
