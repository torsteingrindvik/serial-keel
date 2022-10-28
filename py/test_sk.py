import json
import logging
from pathlib import Path
import re
from typing import Set
from unittest import IsolatedAsyncioTestCase
import unittest

from serial_keel import SerialKeel, connect

logging.basicConfig(format="%(asctime)s     %(message)s", level=logging.INFO)


async def observe(sk: SerialKeel, endpoint: str, mock_file: str = None):
    """
    Start observing ("subscribing") to an endpoint.

    If mock file is given, this should be a relative path to a text file
    from this script's folder.

    In the case of the mock file, the mock file's contents will be
    received line by line.
    """
    if mock_file is not None:
        observer = await sk.observe_mock(endpoint)
        """
        This is a mock session.
        Instead of writing to a serial port, when writing
        to a mock, the data will be sent back to us
        line by line.

        This means we can inject any text we want and check
        our logic against it.
        """
        this_folder = Path(__file__).parent
        with open(this_folder / mock_file) as f:
            await observer.write(f.read())
    else:
        # Not implemented yet
        observer = await sk.observe(endpoint)


async def tfm_test(sk: SerialKeel, to_find: Set[str], allowed_to_fail: Set[str], end_condition: str):
    found_passed = set()
    found_failed = set()

    async for response in sk:
        message = response['message']
        if end_condition in message:
            break

        # A concluding test has the format:
        #       TEST: TFM_SOME_NAME_1234 - PASSED!
        # or
        #       TEST: TFM_SOME_NAME_1234 - FAILED!
        pattern = 'TEST: (.*) - (PASSED|FAILED)!'
        if match := re.search(pattern, message):
            test_name = match.group(1)
            verdict = match.group(2)

            to_find.remove(test_name)
            if verdict == 'PASSED':
                found_passed.add(test_name)
            else:
                found_failed.add(test_name)

    if len(found_failed) != 0:
        for failed in list(found_failed):
            if failed in allowed_to_fail:
                logging.warning(f'Failed but allowed to: {failed}')
            else:
                logging.error(f'Failed: {failed}')
                raise RuntimeError('Not all tests passed')

    if len(to_find) != 0:
        for missing in list(to_find):
            logging.error(f'Not found: {missing}')
        logging.info("sup")
        raise RuntimeError('Not all tests were executed')


class SerialKeelAsyncioTestCase(IsolatedAsyncioTestCase):
    async def test_mock_crypto_test_app(self):
        # TODO: In Python 3.11 there is actually support for
        # async context managers:
        # https://docs.python.org/3/library/unittest.html#unittest.IsolatedAsyncioTestCase.enterAsyncContext
        async with connect("ws://127.0.0.1:3000/ws") as sk:
            """
            Now we are already connected to the server.
            The server program is called SerialKeel; therefore 'sk'.

            Now we choose if we're running a mock session or not:
            """
            await observe(sk, 'mock-crypto-test-app', 'mock/crypto-test-app.txt')

            """
            From this point on our test logic has no idea if the data coming in
            is from a real serial port or a mocked one.
            """
            async for response in sk:  # TODO: Make async iterator which filters on endpoint
                if 'PROJECT EXECUTION SUCCESSFUL' in response['message']:
                    break

    async def test_tfm_regression(self):
        async with connect("ws://127.0.0.1:3000/ws") as sk:
            with open(Path(__file__).parent / 'tfm-spec.json') as f:
                spec = json.loads(f.read())

            # Secure
            secure_spec = spec['secure']
            await observe(sk, 'mock-tfm-regression-secure', 'mock/tfm-regression-secure.txt')
            await tfm_test(sk, set(secure_spec['to_find']), set(secure_spec['allowed_to_fail']), secure_spec['end_condition'])

            # Non-secure
            non_secure_spec = spec['non-secure']
            await observe(sk, 'mock-tfm-regression-non-secure', 'mock/tfm-regression-non-secure.txt')
            await tfm_test(sk, set(non_secure_spec['to_find']), set(non_secure_spec['allowed_to_fail']), non_secure_spec['end_condition'])


if __name__ == "__main__":
    unittest.main()
