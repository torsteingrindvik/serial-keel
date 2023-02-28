import pytest
from pathlib import Path

from serialkeel import connect, make_logger

# See README.md for information on how to run this.

@pytest.mark.asyncio_cooperative
@pytest.mark.parametrize("n", range(10))
async def test_crypto_test_app(n):
    logger = make_logger(f'logger-{n}', add_formatter=True)

    async with connect("ws://127.0.0.1:3123/client", timeout=100, logger=logger) as sk:
        # Here we give a list of labels the endpoint(s) we wish to control must match.
        #
        # This could be something like ['blue', 'kitchen'] for example if the device has to be blue
        # and located in a kitchen.
        endpoints = await sk.control_any(['mocks'])

        logger.info('Controlling endpoints: {endpoints}')

        # It's endpoints (plural) because a group might match the labels given.
        # In that case we get control over _all_ those endpoints at the same time.
        #
        # If no endpoints matched the label, the Serial Keel client raises an exception.
        #
        # Since we know the config (test-concurrent.ron) the server runs with, we know that
        # there are no groups- there will only be one device.
        endpoint = endpoints[0]

        # Actually receiving serial messages the endpoint outputs is opt-in.
        await sk.observe(endpoint)
        logger.info('Observing endpoint')

        # Our endpoint is a mock- it does not actually produce any output.
        # Serial Keel allows mocking by just sending back (line by line) any input
        # a mock endpoint is sent.
        await sk.write_file(endpoint, Path('crypto-test-app.txt'))

        num_messages = 0

        # Using async Python we can simply iterate over messages received on an endpoint.
        async for message in sk.endpoint_messages(endpoint):

            num_messages += 1
            if num_messages % 10 == 0:
                logger.info(f'Messages: {num_messages}')

            if 'PROJECT EXECUTION SUCCESSFUL' in message:
                break
