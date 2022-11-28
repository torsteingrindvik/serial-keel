import pytest
from pathlib import Path

from serial_keel import connect
from serial_keel_util import make_logger

# See README.md for information on how to run this.

@pytest.mark.asyncio_cooperative
@pytest.mark.parametrize("n", range(10))
async def test_crypto_test_app(n):
    logger = make_logger(f'logger-{n}', add_formatter=True)

    async with connect("ws://127.0.0.1:3123/ws", timeout=100, logger=logger) as sk:
        label = 'mocks'

        endpoints = await sk.control_any([label])

        logger.info('Controlling endpoints: {endpoints}')

        # In real situations we may have gotten control over several endpoints,
        # but for us we know there are no grouped endpoints
        endpoint = next(iter(endpoints), None)
        if endpoint is None:
            logger.error(f"No endpoints found")
            return

        await sk.observe(endpoint)
        logger.info('Observing endpoint')

        await sk.write_file(endpoint, Path('mock/crypto-test-app.txt'))

        num_messages = 0
        async for message in sk.endpoint_messages(endpoint):
            num_messages += 1
            if num_messages % 10 == 0:
                logger.info(f'Messages: {num_messages}')

            if 'PROJECT EXECUTION SUCCESSFUL' in message:
                break
