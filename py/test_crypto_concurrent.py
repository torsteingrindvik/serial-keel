import pytest
import logging
from pathlib import Path

from serial_keel import connect


# Note that for mocks to actually share resources,
# we need the `mocks-share-endpoints` feature to be enabled.
# E.g.: `cargo r --features mocks-share-endpoints`

# Note: Pass the `test-concurrent.ron` configuration to the server
# when starting.

@pytest.mark.asyncio_cooperative
@pytest.mark.parametrize("n", range(10))
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

    mock = True  # TODO: Pass via cli

    if mock:
        timeout = 100
    else:
        timeout = 1000

    async with connect("ws://127.0.0.1:3123/ws", timeout, logger) as sk:
        if mock:
            label = 'mocks'
        else:
            label = 'ttys'  # TODO

        endpoints = await sk.control_any([label])

        logger.info('Controlling endpoints: {endpoints}')
        if not mock:
            # To get output going
            import os
            os.system('nrfjprog.exe -r')

        # In real situations we may have gotten control over several endpoints,
        # but for us we know there are no grouped endpoints
        endpoint = endpoints[0]
        await sk.observe(endpoint)
        logger.info('Observing endpoint')

        if mock:
            await sk.write_file(endpoint, Path('mock/crypto-test-app.txt'))

        num_messages = 0
        async for message in sk.endpoint_messages(endpoint):
            num_messages += 1
            if num_messages % 10 == 0:
                logger.info(f'Messages: {num_messages}')

            if 'PROJECT EXECUTION SUCCESSFUL' in message:
                break
