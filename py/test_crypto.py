import logging
from unittest import IsolatedAsyncioTestCase
import unittest
from pathlib import Path

from serial_keel import connect

logging.basicConfig(format="%(asctime)s     %(message)s", level=logging.INFO)


class SerialKeelAsyncioTestCase(IsolatedAsyncioTestCase):
    async def test_crypto_test_app(self):
        # TODO: In Python 3.11 there is actually support for
        # async context managers:
        # https://docs.python.org/3/library/unittest.html#unittest.IsolatedAsyncioTestCase.enterAsyncContext
        async with connect("ws://127.0.0.1:3000/ws") as sk:
            """
            Now we are already connected to the server.
            The server program is called SerialKeel; therefore 'sk'.
            """

            # Mock version:
            endpoint = await sk.observe_mock('mock-crypto-test-app', Path('mock/crypto-test-app.txt'))

            # TTY version:
            # endpoint = await sk.observe('/dev/ttyACM0')

            """
            From this point on our test logic has no idea if the data coming in
            is from a real serial port or a mocked one.
            """
            async for message in sk.endpoint_messages(endpoint):
                if 'PROJECT EXECUTION SUCCESSFUL' in message:
                    break


if __name__ == "__main__":
    unittest.main()
